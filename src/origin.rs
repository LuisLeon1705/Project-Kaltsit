//! Who is writing a memory: this machine (MAC of its physical network adapter and computer
//! name) and the model producing the text. Every memory keeps it, so modules carried between
//! computers and models can always be told apart by origin.

use crate::cthg::model::Source;

/// MAC of the first physical Ethernet or Wi-Fi adapter ("AA-BB-CC-DD-EE-FF"). Virtual adapters
/// (Hyper-V, VirtualBox, VMware, VPNs, Bluetooth) are skipped so the signature stays stable.
#[cfg(windows)]
pub fn mac() -> String {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetAdaptersInfo, IF_TYPE_IEEE80211, IP_ADAPTER_INFO, MIB_IF_TYPE_ETHERNET};
    const VIRTUAL: [&str; 8] = ["virtual", "hyper-v", "vmware", "virtualbox", "vpn", "bluetooth", "loopback", "tap-"];
    unsafe {
        let mut size = 0u32;
        GetAdaptersInfo(std::ptr::null_mut(), &mut size);
        if size == 0 {
            return String::new();
        }
        // u64 storage keeps the buffer aligned for IP_ADAPTER_INFO.
        let mut buf = vec![0u64; size as usize / 8 + 1];
        let head = buf.as_mut_ptr() as *mut IP_ADAPTER_INFO;
        if GetAdaptersInfo(head, &mut size) != 0 {
            return String::new();
        }
        let mut best: Option<(u32, String)> = None;
        let mut p = head;
        while !p.is_null() {
            let a = &*p;
            let desc: String = a.Description.iter().take_while(|c| **c != 0).map(|c| *c as u8 as char).collect::<String>().to_lowercase();
            let physical = !VIRTUAL.iter().any(|v| desc.contains(v));
            let rank = match a.Type {
                MIB_IF_TYPE_ETHERNET => 0,
                IF_TYPE_IEEE80211 => 1,
                _ => 2,
            };
            if a.AddressLength == 6 && physical && rank < 2 && a.Address[..6].iter().any(|b| *b != 0) {
                let m = a.Address[..6].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("-");
                if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                    best = Some((rank, m));
                }
            }
            p = a.Next;
        }
        best.map(|b| b.1).unwrap_or_default()
    }
}

#[cfg(not(windows))]
pub fn mac() -> String {
    String::new()
}

pub fn host() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default()
}

/// This machine with the given model (or "semilla" for hand-written memories).
pub fn here(model: &str) -> Source {
    Source { mac: mac(), host: host(), model: model.to_string() }
}

/// The model file llama-server is serving ("gemma-4-12B-it-Q3_K_XL.gguf"), if it answers.
pub fn served_model(url: &str) -> Option<String> {
    let v: serde_json::Value = ureq::get(&format!("{url}/v1/models")).timeout(std::time::Duration::from_secs(3)).call().ok()?.into_json().ok()?;
    let id = v["data"][0]["id"].as_str().or_else(|| v["models"][0]["model"].as_str())?;
    let name = id.rsplit(['/', '\\']).next().unwrap_or(id);
    Some(name.to_string())
}
