//! Wi-Fi setup from the tablet, through NetworkManager's `nmcli`, so a Pi on
//! its fallback hotspot can be moved onto a new network without SSH.
//!
//! `nmcli` is always run with an argument list (never a shell), and the SSID
//! and password are validated first.

use std::process::Command;

use serde::Serialize;

const IFACE: &str = "wlan0";

#[derive(Serialize, Debug, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub signal: u8,
    pub secure: bool,
    /// The Pi is connected to it right now.
    pub active: bool,
}

/// Splits one line of `nmcli -t` output, where `:` and `\` in values are
/// escaped as `\:` and `\\`.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => fields.last_mut().unwrap().extend(chars.next()),
            ':' => fields.push(String::new()),
            c => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

/// Parses `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY dev wifi list`: one entry
/// per SSID (strongest signal wins), hidden networks skipped, strongest first.
fn parse_scan(out: &str) -> Vec<Network> {
    let mut nets: Vec<Network> = Vec::new();
    for f in out.lines().map(split_terse).filter(|f| f.len() == 4) {
        let (ssid, signal) = (f[1].clone(), f[2].parse().unwrap_or(0));
        if ssid.is_empty() {
            continue;
        }
        let net = Network { secure: !matches!(f[3].as_str(), "" | "--"), active: f[0] == "*", signal, ssid };
        match nets.iter_mut().find(|n| n.ssid == net.ssid) {
            Some(n) => {
                n.active |= net.active;
                n.signal = n.signal.max(net.signal);
            }
            None => nets.push(net),
        }
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets
}

/// WPA rules: SSID 1–32 bytes, password empty (open network) or 8–63 chars.
pub fn validate(ssid: &str, password: &str) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("network name must be 1–32 characters".into());
    }
    if ssid.chars().any(char::is_control) || password.chars().any(char::is_control) {
        return Err("network name and password can't contain control characters".into());
    }
    if !password.is_empty() && !(8..=63).contains(&password.chars().count()) {
        return Err("password must be 8–63 characters (or empty for an open network)".into());
    }
    Ok(())
}

fn nmcli(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli").args(args).output().map_err(|e| format!("nmcli: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[derive(Serialize)]
pub struct Current {
    pub ssid: Option<String>,
    /// The Pi is running its own fallback hotspot.
    pub hotspot: bool,
}

/// The network `wlan0` is on now, without scanning.
pub fn current() -> Result<Current, String> {
    let name = nmcli(&["-g", "GENERAL.CONNECTION", "device", "show", IFACE])?.trim().to_string();
    if name.is_empty() || name == "--" {
        return Ok(Current { ssid: None, hotspot: false });
    }
    let field = |f: &str| nmcli(&["-g", f, "connection", "show", "id", &name]).map(|s| s.trim().to_string());
    Ok(Current { ssid: Some(field("802-11-wireless.ssid")?), hotspot: field("802-11-wireless.mode")? == "ap" })
}

pub fn scan() -> Result<Vec<Network>, String> {
    // A radio that's running the hotspot may not be able to scan; the
    // tablet then still lets you type a network name.
    nmcli(&["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "auto"]).map(|o| parse_scan(&o))
}

/// Saves `ssid` as a profile (power save off, like install.sh) and switches
/// to it. Blocks until NetworkManager reports success or failure; on failure
/// NetworkManager falls back to a known network or the hotspot by itself.
pub fn connect(ssid: &str, password: &str) -> Result<(), String> {
    validate(ssid, password)?;
    // Re-entering a network replaces its old profile (e.g. a changed password).
    if nmcli(&["-t", "-f", "NAME", "connection", "show"])?.lines().any(|n| n == ssid) {
        nmcli(&["connection", "delete", "id", ssid])?;
    }
    let mut args = vec!["connection", "add", "type", "wifi", "ifname", IFACE, "con-name", ssid, "ssid", ssid];
    args.extend(["802-11-wireless.powersave", "2", "connection.autoconnect", "yes"]);
    if !password.is_empty() {
        args.extend(["wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", password]);
    }
    nmcli(&args)?;
    nmcli(&["connection", "up", "id", ssid]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scan_output() {
        let out = "*:Home:72:WPA2\n:Home:40:WPA2\n:Cafe\\: Free:55:\n::80:WPA2\n:Back\\\\slash:30:WPA1 WPA2\n";
        let nets = parse_scan(out);
        assert_eq!(nets.len(), 3, "duplicate SSID merged, hidden network skipped");
        assert_eq!(nets[0], Network { ssid: "Home".into(), signal: 72, secure: true, active: true });
        assert_eq!(nets[1], Network { ssid: "Cafe: Free".into(), signal: 55, secure: false, active: false });
        assert_eq!(nets[2].ssid, "Back\\slash");
    }

    #[test]
    fn validates_credentials() {
        assert!(validate("Home", "longenough").is_ok());
        assert!(validate("Cafe", "").is_ok(), "open network");
        assert!(validate("", "longenough").is_err());
        assert!(validate(&"x".repeat(33), "longenough").is_err());
        assert!(validate("Home", "short").is_err());
        assert!(validate("Home", &"p".repeat(64)).is_err());
        assert!(validate("Home\n", "longenough").is_err());
    }
}
