//! The machine's own model name, from DMI or the device tree.

use super::read_trimmed;

/// Values firmware ships that carry no information.
const PLACEHOLDERS: [&str; 12] = [
    "to be filled by o.e.m.",
    "to be filled by o.e.m",
    "system product name",
    "system version",
    "default string",
    "not specified",
    "not applicable",
    "none",
    "n/a",
    "invalid",
    "unknown",
    "oem",
];

fn meaningful(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || PLACEHOLDERS.contains(&trimmed.to_ascii_lowercase().as_str()) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// The best available description of the machine, e.g. `ThinkPad X1 Carbon Gen 9`.
pub fn model() -> String {
    const DMI: &str = "/sys/devices/virtual/dmi/id";

    let product = read_trimmed(format!("{DMI}/product_name")).and_then(|v| meaningful(&v));
    let version = read_trimmed(format!("{DMI}/product_version")).and_then(|v| meaningful(&v));

    if let Some(product) = product {
        // Lenovo puts the marketing name in product_version and a model code
        // in product_name, so both are worth showing when they differ.
        return match version {
            Some(v) if !product.contains(&v) && !v.contains(&product) => format!("{product} {v}"),
            _ => product,
        };
    }

    // Boards without a product name still have a vendor and board model.
    let vendor = read_trimmed(format!("{DMI}/sys_vendor")).and_then(|v| meaningful(&v));
    let board = read_trimmed(format!("{DMI}/board_name")).and_then(|v| meaningful(&v));
    if let (Some(vendor), Some(board)) = (&vendor, &board) {
        return format!("{vendor} {board}");
    }

    // ARM boards describe themselves in the device tree instead.
    if let Some(dt) = read_trimmed("/sys/firmware/devicetree/base/model").and_then(|v| {
        // Device tree strings are NUL-terminated.
        meaningful(v.trim_end_matches('\0'))
    }) {
        return dt;
    }

    vendor.or(board).unwrap_or_else(|| "Unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_rejected() {
        assert_eq!(meaningful("To Be Filled By O.E.M."), None);
        assert_eq!(meaningful("Default string"), None);
        assert_eq!(meaningful("  "), None);
        assert_eq!(meaningful("None"), None);
    }

    #[test]
    fn real_values_survive() {
        assert_eq!(meaningful(" 20XW "), Some("20XW".to_string()));
        assert_eq!(meaningful("MS-7C91"), Some("MS-7C91".to_string()));
    }

    #[test]
    fn model_always_returns_something() {
        assert!(!model().is_empty());
    }
}
