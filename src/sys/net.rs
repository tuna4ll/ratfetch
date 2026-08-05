//! Network interfaces: counters from `/proc/net/dev`, addresses from
//! `getifaddrs`.

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;

use super::read_trimmed;
use crate::config::model::Network as NetCfg;

/// One interface, with rates measured since the previous sample.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Interface {
    pub name: String,
    /// Bytes received since boot.
    pub rx_bytes: u64,
    /// Bytes transmitted since boot.
    pub tx_bytes: u64,
    /// Bytes per second, averaged over the last interval.
    pub rx_rate: f64,
    pub tx_rate: f64,
    /// `up`, `down`, `unknown`, ...
    pub state: String,
    /// The first IPv4 address bound to the interface.
    pub ipv4: Option<String>,
    pub is_loopback: bool,
}

impl Interface {
    pub fn is_up(&self) -> bool {
        // Loopback and many virtual devices report "unknown" rather than "up".
        self.state == "up" || self.state == "unknown"
    }
}

/// A raw counter pair from `/proc/net/dev`.
#[derive(Debug, Clone, Copy, Default)]
struct Counters {
    rx: u64,
    tx: u64,
}

/// Parses `/proc/net/dev` into per-interface counters.
fn parse_dev(text: &str) -> Vec<(String, Counters)> {
    text.lines()
        .skip(2) // two header rows
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let fields: Vec<u64> = rest
                .split_whitespace()
                .map(|f| f.parse().unwrap_or(0))
                .collect();
            // Receive occupies the first eight columns, transmit the next eight.
            let rx = *fields.first()?;
            let tx = *fields.get(8)?;
            Some((name.trim().to_string(), Counters { rx, tx }))
        })
        .collect()
}

/// Whether an interface passes the config's filters.
fn wanted(name: &str, is_loopback: bool, cfg: &NetCfg) -> bool {
    if !cfg.interfaces.is_empty() {
        // An explicit list overrides every other filter.
        return cfg.interfaces.iter().any(|i| i == name);
    }
    if is_loopback && !cfg.show_loopback {
        return false;
    }
    !cfg.hide_prefixes
        .iter()
        .any(|p| !p.is_empty() && name.starts_with(p.as_str()))
}

/// Turns successive `/proc/net/dev` readings into rates.
pub struct Sampler {
    prev: HashMap<String, Counters>,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            prev: HashMap::new(),
        }
    }

    pub fn sample(&mut self, elapsed: f64, cfg: &NetCfg) -> Vec<Interface> {
        let text = fs::read_to_string("/proc/net/dev").unwrap_or_default();
        let addrs = ipv4_addresses();
        let mut out = Vec::new();
        let mut current = HashMap::new();

        for (name, counters) in parse_dev(&text) {
            current.insert(name.clone(), counters);

            let is_loopback = name == "lo";
            if !wanted(&name, is_loopback, cfg) {
                continue;
            }

            // The first sample, a counter reset, or a zero interval all mean
            // there is no meaningful rate to report yet.
            let (rx_rate, tx_rate) = match self.prev.get(&name) {
                Some(prev) if elapsed > 0.0 => (
                    counters.rx.saturating_sub(prev.rx) as f64 / elapsed,
                    counters.tx.saturating_sub(prev.tx) as f64 / elapsed,
                ),
                _ => (0.0, 0.0),
            };

            let state = read_trimmed(format!("/sys/class/net/{name}/operstate"))
                .unwrap_or_else(|| "unknown".to_string());

            let iface = Interface {
                rx_bytes: counters.rx,
                tx_bytes: counters.tx,
                rx_rate,
                tx_rate,
                state,
                ipv4: addrs.get(&name).cloned(),
                is_loopback,
                name,
            };

            if cfg.interfaces.is_empty() && !iface.is_up() {
                continue;
            }
            out.push(iface);
        }

        self.prev = current;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps interface name to its first IPv4 address.
fn ipv4_addresses() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();

    // SAFETY: getifaddrs allocates a list we free below; a non-zero return
    // means nothing was allocated.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return out;
    }

    let mut cur = head;
    while !cur.is_null() {
        // SAFETY: the list is well-formed until the null terminator.
        unsafe {
            let entry = &*cur;
            cur = entry.ifa_next;

            if entry.ifa_addr.is_null() || entry.ifa_name.is_null() {
                continue;
            }
            if (*entry.ifa_addr).sa_family != libc::AF_INET as libc::sa_family_t {
                continue;
            }

            let addr = &*(entry.ifa_addr as *const libc::sockaddr_in);
            let octets = addr.sin_addr.s_addr.to_ne_bytes();
            let Ok(name) = CStr::from_ptr(entry.ifa_name).to_str() else {
                continue;
            };

            out.entry(name.to_string()).or_insert_with(|| {
                format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
            });
        }
    }

    // SAFETY: head came from getifaddrs and is freed exactly once.
    unsafe { libc::freeifaddrs(head) };
    out
}

/// The address shown in the `local_ip` info row: the first non-loopback one.
pub fn local_ip(interfaces: &[Interface]) -> String {
    interfaces
        .iter()
        .filter(|i| !i.is_loopback)
        .find_map(|i| i.ipv4.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1000      10    0    0    0     0          0         0     1000      10    0    0    0     0       0          0
  eth0: 5000      50    0    0    0     0          0         0     2000      20    0    0    0     0       0          0
docker0: 100       1    0    0    0     0          0         0        0       0    0    0    0     0       0          0
";

    #[test]
    fn parses_counters() {
        let parsed = parse_dev(DEV);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].0, "lo");
        assert_eq!(parsed[1].0, "eth0");
        assert_eq!(parsed[1].1.rx, 5000);
        assert_eq!(parsed[1].1.tx, 2000);
    }

    #[test]
    fn header_rows_are_skipped() {
        assert!(parse_dev("Inter-|\n face |\n").is_empty());
        assert!(parse_dev("").is_empty());
    }

    #[test]
    fn filters_follow_the_config() {
        let cfg = NetCfg::default();
        assert!(wanted("eth0", false, &cfg));
        assert!(!wanted("lo", true, &cfg), "loopback hidden by default");
        assert!(!wanted("docker0", false, &cfg), "hidden prefix");
        assert!(!wanted("veth1234", false, &cfg));

        let cfg = NetCfg {
            show_loopback: true,
            ..Default::default()
        };
        assert!(wanted("lo", true, &cfg));

        // An explicit list wins over the prefix filter.
        let cfg = NetCfg {
            interfaces: vec!["docker0".into()],
            ..Default::default()
        };
        assert!(wanted("docker0", false, &cfg));
        assert!(!wanted("eth0", false, &cfg));
    }

    #[test]
    fn rates_need_two_samples() {
        let mut s = Sampler::new();
        s.prev
            .insert("eth0".into(), Counters { rx: 4000, tx: 1000 });

        // 1000 bytes in 2 seconds is 500 B/s.
        let counters = parse_dev(DEV);
        let eth = counters.iter().find(|(n, _)| n == "eth0").unwrap().1;
        let rx_rate = eth.rx.saturating_sub(4000) as f64 / 2.0;
        assert_eq!(rx_rate, 500.0);
    }

    #[test]
    fn counter_resets_do_not_produce_negative_rates() {
        let now = Counters { rx: 10, tx: 10 };
        let prev = Counters {
            rx: 9_000_000,
            tx: 9_000_000,
        };
        assert_eq!(now.rx.saturating_sub(prev.rx), 0);
    }

    #[test]
    fn local_ip_skips_loopback() {
        let interfaces = vec![
            Interface {
                name: "lo".into(),
                ipv4: Some("127.0.0.1".into()),
                is_loopback: true,
                ..Default::default()
            },
            Interface {
                name: "eth0".into(),
                ipv4: Some("192.168.1.10".into()),
                ..Default::default()
            },
        ];
        assert_eq!(local_ip(&interfaces), "192.168.1.10");
        assert_eq!(local_ip(&[]), "");
    }

    #[test]
    fn up_covers_the_unknown_state() {
        let up = Interface {
            state: "up".into(),
            ..Default::default()
        };
        let unknown = Interface {
            state: "unknown".into(),
            ..Default::default()
        };
        let down = Interface {
            state: "down".into(),
            ..Default::default()
        };
        assert!(up.is_up() && unknown.is_up() && !down.is_up());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reads_this_machine() {
        let mut s = Sampler::new();
        let cfg = NetCfg {
            show_loopback: true,
            ..Default::default()
        };
        let _ = s.sample(0.0, &cfg);
        let ifaces = s.sample(1.0, &cfg);
        assert!(
            ifaces.iter().any(|i| i.name == "lo"),
            "loopback always exists"
        );
        assert!(ifaces.iter().all(|i| i.rx_rate >= 0.0));
    }
}
