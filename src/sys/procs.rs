//! The process table, from `/proc/<pid>`.

use std::collections::HashMap;
use std::fs;

use crate::config::enums::ProcSort;
use crate::config::model::Processes as ProcCfg;

/// One process, with CPU measured over the last interval.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Process {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
    /// The full command line, or the name again when it is unreadable.
    pub command: String,
    /// Single-letter state: `R`, `S`, `D`, `Z`, ...
    pub state: char,
    /// Percentage of one CPU used since the previous sample.
    pub cpu: f64,
    /// Resident set size, in bytes.
    pub memory: u64,
    pub threads: u64,
    pub uid: u32,
}

impl Process {
    /// What the process table shows in its command column.
    pub fn display(&self, full: bool) -> &str {
        if full && !self.command.is_empty() {
            &self.command
        } else {
            &self.name
        }
    }
}

/// The fields of `/proc/<pid>/stat` this program uses.
struct Stat {
    name: String,
    state: char,
    ppid: i32,
    ticks: u64,
    rss_pages: u64,
    threads: u64,
}

/// Parses `/proc/<pid>/stat`.
///
/// The `comm` field is wrapped in parentheses and may itself contain spaces
/// and parentheses, so everything after the *last* `)` is what gets split.
fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?.to_string();

    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    // fields[0] is `state`, i.e. field 3 of the file, so field N is at N - 3.
    let get = |n: usize| -> u64 { fields.get(n - 3).and_then(|f| f.parse().ok()).unwrap_or(0) };

    Some(Stat {
        name,
        state: fields.first().and_then(|s| s.chars().next()).unwrap_or('?'),
        ppid: fields.get(1).and_then(|f| f.parse().ok()).unwrap_or(0),
        ticks: get(14) + get(15), // utime + stime
        rss_pages: get(24),
        threads: get(20),
    })
}

/// `/proc/<pid>/cmdline` is NUL-separated.
fn read_cmdline(pid: i32) -> String {
    let Ok(raw) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&raw);
    text.split('\0')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The owning uid, from `/proc/<pid>/status`.
fn read_uid(pid: i32) -> u32 {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|text| {
            let line = text.lines().find(|l| l.starts_with("Uid:"))?;
            // Uid: real effective saved fs
            line.split_whitespace().nth(1)?.parse().ok()
        })
        .unwrap_or(0)
}

/// Turns successive `/proc` walks into per-process CPU percentages.
pub struct Sampler {
    prev: HashMap<i32, u64>,
    ticks_per_sec: f64,
    page_size: u64,
    uid: u32,
    /// Processes seen in the last sample, before filtering.
    pub last_total: usize,
    /// Threads across those processes.
    pub last_threads: usize,
}

impl Sampler {
    pub fn new() -> Self {
        // SAFETY: sysconf reads static system parameters.
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        Self {
            prev: HashMap::new(),
            ticks_per_sec: if ticks > 0 { ticks as f64 } else { 100.0 },
            page_size: if page > 0 { page as u64 } else { 4096 },
            uid: unsafe { libc::geteuid() },
            last_total: 0,
            last_threads: 0,
        }
    }

    pub fn sample(&mut self, elapsed: f64, cfg: &ProcCfg) -> Vec<Process> {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };

        let mut current = HashMap::new();
        let mut out = Vec::new();
        let mut total = 0usize;
        let mut threads = 0usize;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.parse::<i32>().ok()) else {
                continue;
            };

            // A process can exit between the readdir and the read; that is
            // routine, not an error.
            let Ok(text) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            let Some(stat) = parse_stat(&text) else {
                continue;
            };

            total += 1;
            threads += stat.threads as usize;
            current.insert(pid, stat.ticks);

            // A kernel thread has no command line at all.
            let command = read_cmdline(pid);
            if cfg.hide_kthreads && command.is_empty() {
                continue;
            }

            let uid = read_uid(pid);
            if cfg.only_mine && uid != self.uid {
                continue;
            }

            let cpu = match self.prev.get(&pid) {
                Some(prev) if elapsed > 0.0 => {
                    let delta = stat.ticks.saturating_sub(*prev) as f64;
                    (delta / self.ticks_per_sec / elapsed * 100.0).clamp(0.0, 100.0 * 512.0)
                }
                _ => 0.0,
            };

            out.push(Process {
                pid,
                ppid: stat.ppid,
                name: stat.name,
                command,
                state: stat.state,
                cpu,
                memory: stat.rss_pages.saturating_mul(self.page_size),
                threads: stat.threads,
                uid,
            });
        }

        self.prev = current;
        self.last_total = total;
        self.last_threads = threads;

        sort(&mut out, cfg.sort, cfg.ascending);
        out
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Orders the table by the configured column.
pub fn sort(procs: &mut [Process], by: ProcSort, ascending: bool) {
    procs.sort_by(|a, b| {
        let ord = match by {
            // Ties broken by pid so the order does not jitter between ticks.
            ProcSort::Cpu => a.cpu.total_cmp(&b.cpu).then(a.pid.cmp(&b.pid)),
            ProcSort::Memory => a.memory.cmp(&b.memory).then(a.pid.cmp(&b.pid)),
            ProcSort::Pid => a.pid.cmp(&b.pid),
            ProcSort::Name => a
                .name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.pid.cmp(&b.pid)),
        };
        if ascending {
            ord
        } else {
            ord.reverse()
        }
    });
}

/// Processes prepared for the interactive table, paired with their tree depth.
pub fn view<'a>(procs: &'a [Process], query: &str, tree: bool) -> Vec<(&'a Process, usize)> {
    let query = query.trim().to_ascii_lowercase();
    let matches = |process: &Process| {
        query.is_empty()
            || process.name.to_ascii_lowercase().contains(&query)
            || process.command.to_ascii_lowercase().contains(&query)
            || process.pid.to_string().contains(&query)
    };

    if !tree {
        return procs
            .iter()
            .filter(|process| matches(process))
            .map(|process| (process, 0))
            .collect();
    }

    use std::collections::{HashMap, HashSet};
    let known: HashSet<i32> = procs.iter().map(|process| process.pid).collect();
    let mut children: HashMap<i32, Vec<&Process>> = HashMap::new();
    let mut roots = Vec::new();
    for process in procs {
        if process.ppid > 0 && known.contains(&process.ppid) && process.ppid != process.pid {
            children.entry(process.ppid).or_default().push(process);
        } else {
            roots.push(process);
        }
    }

    fn walk<'a>(
        process: &'a Process,
        depth: usize,
        children: &HashMap<i32, Vec<&'a Process>>,
        seen: &mut HashSet<i32>,
        out: &mut Vec<(&'a Process, usize)>,
    ) {
        if !seen.insert(process.pid) {
            return;
        }
        out.push((process, depth));
        if let Some(nodes) = children.get(&process.pid) {
            for child in nodes {
                walk(child, depth.saturating_add(1), children, seen, out);
            }
        }
    }

    let mut ordered = Vec::with_capacity(procs.len());
    let mut seen = HashSet::new();
    for root in roots {
        walk(root, 0, &children, &mut seen, &mut ordered);
    }
    // Malformed parent cycles are still shown once.
    for process in procs {
        walk(process, 0, &children, &mut seen, &mut ordered);
    }
    ordered
        .into_iter()
        .filter(|(process, _)| matches(process))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real line, with the numbers after `)` in their true positions.
    const STAT: &str = "1234 (my prog) S 1 1234 1234 0 -1 4194560 1000 0 0 0 \
150 50 0 0 20 0 7 0 12345 123456789 2048 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 4 0 0";

    #[test]
    fn parses_stat_with_spaces_in_the_name() {
        let s = parse_stat(STAT).unwrap();
        assert_eq!(s.name, "my prog");
        assert_eq!(s.state, 'S');
        assert_eq!(s.ppid, 1);
        assert_eq!(s.ticks, 200, "utime 150 + stime 50");
        assert_eq!(s.threads, 7);
        assert_eq!(s.rss_pages, 2048);
    }

    #[test]
    fn parses_stat_with_parentheses_in_the_name() {
        let text = STAT.replace("(my prog)", "((weird) name)");
        let s = parse_stat(&text).unwrap();
        assert_eq!(s.name, "(weird) name");
        assert_eq!(s.ticks, 200, "field offsets survive the extra parens");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_stat("").is_none());
        assert!(parse_stat("no parens here").is_none());
    }

    fn p(pid: i32, name: &str, cpu: f64, memory: u64) -> Process {
        Process {
            pid,
            name: name.into(),
            cpu,
            memory,
            ..Default::default()
        }
    }

    #[test]
    fn sorts_descending_by_default() {
        let mut procs = vec![
            p(1, "a", 5.0, 100),
            p(2, "b", 50.0, 10),
            p(3, "c", 1.0, 999),
        ];

        sort(&mut procs, ProcSort::Cpu, false);
        assert_eq!(
            procs.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );

        sort(&mut procs, ProcSort::Memory, false);
        assert_eq!(
            procs.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![3, 1, 2]
        );

        sort(&mut procs, ProcSort::Name, true);
        assert_eq!(
            procs.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        sort(&mut procs, ProcSort::Pid, true);
        assert_eq!(
            procs.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn equal_values_keep_a_stable_order() {
        let mut procs = vec![p(3, "a", 0.0, 0), p(1, "b", 0.0, 0), p(2, "c", 0.0, 0)];
        sort(&mut procs, ProcSort::Cpu, true);
        assert_eq!(
            procs.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn display_falls_back_to_the_name() {
        let mut proc = p(1, "prog", 0.0, 0);
        assert_eq!(proc.display(false), "prog");
        assert_eq!(proc.display(true), "prog", "no command line to show");
        proc.command = "/usr/bin/prog --flag".into();
        assert_eq!(proc.display(true), "/usr/bin/prog --flag");
    }

    #[test]
    fn view_filters_and_builds_a_tree() {
        let mut parent = p(10, "shell", 0.0, 0);
        parent.ppid = 1;
        let mut child = p(20, "worker", 0.0, 0);
        child.ppid = 10;
        let procs = vec![child, parent];

        let tree = view(&procs, "", true);
        assert_eq!(
            tree.iter().map(|(p, d)| (p.pid, *d)).collect::<Vec<_>>(),
            vec![(10, 0), (20, 1)]
        );
        assert_eq!(view(&procs, "work", false)[0].0.pid, 20);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reads_this_machine() {
        let cfg = ProcCfg::default();
        let mut s = Sampler::new();
        let _ = s.sample(0.0, &cfg);
        let procs = s.sample(1.0, &cfg);

        assert!(!procs.is_empty(), "at least this process should be listed");
        assert!(s.last_total >= procs.len());
        assert!(procs.iter().any(|p| p.pid == std::process::id() as i32));
        assert!(procs.iter().all(|p| p.cpu >= 0.0));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn kernel_threads_can_be_hidden() {
        let mut s = Sampler::new();
        let with = s.sample(
            0.0,
            &ProcCfg {
                hide_kthreads: false,
                ..Default::default()
            },
        );
        let mut s = Sampler::new();
        let without = s.sample(
            0.0,
            &ProcCfg {
                hide_kthreads: true,
                ..Default::default()
            },
        );
        assert!(without.len() <= with.len());
    }
}
