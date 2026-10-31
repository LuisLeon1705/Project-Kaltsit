//! The link between computers on the same network, between **linked** computers only.
//!
//! - Identity: each computer has its own X25519 key pair (in `local.json`, never shared). Two
//!   computers are linked once, by hand, on both screens: each shows the same 6-digit code
//!   (computed from both public keys) and the user accepts on both. Someone in the middle would
//!   make the codes differ. Then each keeps the other's public key.
//! - Sessions with forward secrecy: before talking, two linked computers exchange one-use X25519
//!   keys, sealed with the key of the pair (so nobody else can slip in theirs). The session key
//!   comes from those one-use keys, which are destroyed at once; it lives only in memory and is
//!   renewed every ten minutes. Whoever steals a computer's identity later cannot read what was
//!   recorded before.
//! - Every message is sealed with ChaCha20-Poly1305 under the session key: beacons, requests and
//!   replies. Sender, recipient and time are authenticated; anything older than a minute or seen
//!   before is dropped. Path and body travel inside, so nothing about her (screens, memory, chat)
//!   is readable on the network. Without a session yet, a beacon says only where to reach it.
//! - A computer that is not linked sees nothing and gets nothing, even on the same network.
//!   While "Vincular" is open (two minutes) a computer also announces itself in clear (name and
//!   public key only), so the other one can offer it.
//! - Only private networks: beacons go out and requests come in only through networks Windows
//!   marks as private (home), unless the user allows public ones.

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::ChaCha20Poly1305;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

pub const PROTOCOL: u32 = 3;
/// A computer not heard from for this long is gone.
pub const ALIVE_S: f64 = 8.0;
const BEACON_S: u64 = 2;
/// Messages older (or newer) than this are not accepted: clocks of two home computers agree.
const MAX_SKEW_S: f64 = 60.0;
/// How long "Vincular" stays open.
pub const PAIRING_S: f64 = 120.0;
/// A session key is renewed after this long; an older one is still accepted for a while.
const SESSION_S: f64 = 600.0;
/// Sessions kept per computer (the newest is used; the others still open late messages).
const SESSIONS_KEPT: usize = 3;
/// Between two attempts to open a session with the same computer.
const RETRY_S: f64 = 5.0;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn now() -> f64 {
    crate::clock::unix_now()
}

/// A new identity: the secret half, as hex for `local.json`.
pub fn new_identity() -> String {
    hex(StaticSecret::random_from_rng(OsRng).as_bytes())
}

/// The 6-digit code both screens show while linking two public keys (same on both sides).
pub fn pairing_code(a: &[u8; 32], b: &[u8; 32]) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let h = Sha256::new().chain_update(b"kaltsit-vincular").chain_update(lo).chain_update(hi).finalize();
    let n = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

/// Short fingerprint of a public key, to tell devices apart in the list.
pub fn fingerprint(pk: &[u8; 32]) -> String {
    let h = Sha256::digest(pk);
    format!("{}-{}", hex(&h[..2]), hex(&h[2..4]))
}

/// The fingerprint of a public key kept as hex.
pub fn fingerprint_of(public_hex: &str) -> String {
    pk_of(public_hex).map(|p| fingerprint(&p)).unwrap_or_default()
}

/// A linked computer, as `local.json` keeps it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Vinculo {
    pub id: String,
    pub nombre: String,
    /// Its public key, hex.
    pub publica: String,
    /// Both sides accepted (until then it is dropped when "Vincular" closes).
    pub confirmado: bool,
}

/// A session with one linked computer: its key never touches the disk.
#[derive(Clone)]
struct Session {
    sid: [u8; 8],
    key: ChaCha20Poly1305,
    created: f64,
}

struct Trusted {
    v: Vinculo,
    pk: [u8; 32],
    /// The key of the pair (from both identities): seals only the session handshake and the
    /// beacon that says where to reach this computer.
    key: ChaCha20Poly1305,
    /// Its raw bytes, to bind each session to this pair.
    bind: [u8; 32],
    /// Newest first.
    sessions: Vec<Session>,
    /// Last time a session was asked for (so a computer that does not answer is not hammered).
    tried: f64,
    /// Where it said it can be reached, before there is a session.
    addr: Option<String>,
}

impl Trusted {
    fn fresh_session(&self) -> Option<&Session> {
        self.sessions.first().filter(|s| now() - s.created < SESSION_S)
    }

    fn session(&self, sid: &[u8]) -> Option<&Session> {
        self.sessions.iter().find(|s| s.sid == sid)
    }
}

/// What this computer says about itself in each beacon.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub brain: bool,
    /// When it became her brain (unix): with two brains, the older one stays.
    pub brain_since: f64,
    pub think: bool,
    pub body: bool,
    pub left: String,
    pub right: String,
}

/// Inside a beacon sealed with a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Beacon {
    v: u32,
    id: String,
    name: String,
    port: u16,
    idle: f64,
    t: f64,
    #[serde(flatten)]
    status: Status,
}

/// Inside a beacon sealed with the pair key: only where to open a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Reach {
    v: u32,
    id: String,
    port: u16,
    t: f64,
}

/// What travels on the network: sealed for one recipient, or a clear pairing announcement.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "k")]
enum Datagram {
    /// `s`: the session it is sealed with; none: sealed with the pair key (a `Reach`).
    #[serde(rename = "kaltsit3")]
    Sealed {
        de: String,
        para: String,
        #[serde(default)]
        s: Option<String>,
        c: String,
    },
    #[serde(rename = "kaltsit3-vincular")]
    Pairing { id: String, name: String, port: u16, pk: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct Peer {
    pub id: String,
    pub name: String,
    /// ip:port of its HTTP link.
    pub addr: String,
    pub seen: f64,
    pub idle: f64,
    #[serde(flatten)]
    pub status: Status,
}

/// A computer offering itself while "Vincular" is open.
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub pk: String,
    pub huella: String,
    #[serde(skip)]
    seen: f64,
}

/// A linking in progress: the code to compare and who has accepted.
#[derive(Clone, Debug, Serialize)]
pub struct Linking {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub pk: String,
    pub codigo: String,
    /// The other computer asked (true) or this one did.
    pub entrante: bool,
    pub acepte: bool,
    /// Both accepted.
    pub listo: bool,
    pub rechazado: bool,
}

pub struct Net {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub disc: u16,
    secret: StaticSecret,
    public: [u8; 32],
    trusted: Mutex<Vec<Trusted>>,
    /// Trust list changed: the app writes it to local.json.
    dirty: AtomicBool,
    pub status: Mutex<Status>,
    peers: Mutex<HashMap<String, Peer>>,
    extra: Vec<String>,
    /// Until when "Vincular" is open (unix), 0 if closed.
    pairing_until: Mutex<f64>,
    candidates: Mutex<HashMap<String, Candidate>>,
    linking: Mutex<Option<Linking>>,
    /// Nonces of the last few minutes: a message seen once is not accepted again.
    seen: Mutex<HashMap<[u8; 12], f64>>,
    /// Networks the link may use: (address, prefix) of private interfaces; None = unknown.
    private: Mutex<Option<Vec<(Ipv4Addr, u8)>>>,
    /// Name and category of each connected network, as Windows sees them.
    profiles: Mutex<Vec<(String, String)>>,
    pub allow_public: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn pk_of(hexkey: &str) -> Option<[u8; 32]> {
    unhex(hexkey)?.try_into().ok()
}

fn mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix.min(32) as u32)
    }
}

fn in_net(ip: Ipv4Addr, net: Ipv4Addr, prefix: u8) -> bool {
    u32::from(ip) & mask(prefix) == u32::from(net) & mask(prefix)
}

fn broadcast_of(net: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(net) | !mask(prefix))
}

fn powershell(script: &str) -> Option<String> {
    let mut c = std::process::Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// IPv4 address and prefix of every connected interface Windows does not mark as public.
pub fn private_networks() -> Option<Vec<(Ipv4Addr, u8)>> {
    let out = powershell(
        "Get-NetConnectionProfile | Where-Object { $_.NetworkCategory -ne 'Public' } | \
         ForEach-Object { Get-NetIPAddress -AddressFamily IPv4 -InterfaceIndex $_.InterfaceIndex -ErrorAction SilentlyContinue } | \
         ForEach-Object { \"$($_.IPAddress)/$($_.PrefixLength)\" }",
    )?;
    Some(
        out.lines()
            .filter_map(|l| {
                let (ip, p) = l.trim().split_once('/')?;
                Some((ip.parse().ok()?, p.parse().ok()?))
            })
            .collect(),
    )
}

/// Names and categories (Private / Public / DomainAuthenticated) of the connected networks.
pub fn network_profiles() -> Vec<(String, String)> {
    powershell("Get-NetConnectionProfile | ForEach-Object { \"$($_.Name)`t$($_.NetworkCategory)\" }")
        .map(|o| o.lines().filter_map(|l| l.split_once('\t').map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))).collect())
        .unwrap_or_default()
}

/// The session key from the one-use exchange, bound to the pair and to both one-use keys.
fn session_from(shared: &[u8; 32], bind: &[u8; 32], initiator: &[u8; 32], responder: &[u8; 32]) -> Session {
    let mut info = b"kaltsit-sesion-v3".to_vec();
    info.extend_from_slice(initiator);
    info.extend_from_slice(responder);
    let mut okm = [0u8; 32];
    hkdf::Hkdf::<Sha256>::new(Some(bind), shared).expand(&info, &mut okm).expect("32 bytes");
    let h = Sha256::new().chain_update(initiator).chain_update(responder).finalize();
    Session { sid: h[..8].try_into().unwrap(), key: ChaCha20Poly1305::new(&okm.into()), created: now() }
}

impl Net {
    /// `identity`: this computer's secret (hex, from local.json). `linked`: its trusted list.
    pub fn new(id: String, name: String, port: u16, disc: u16, identity: &str, linked: &[Vinculo], extra: Vec<String>) -> Arc<Net> {
        let bytes: [u8; 32] = unhex(identity).and_then(|b| b.try_into().ok()).unwrap_or_else(|| *StaticSecret::random_from_rng(OsRng).as_bytes());
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret).to_bytes();
        let net = Arc::new(Net {
            id,
            name,
            port,
            disc,
            secret,
            public,
            trusted: Mutex::new(Vec::new()),
            dirty: AtomicBool::new(false),
            status: Mutex::new(Status::default()),
            peers: Mutex::new(HashMap::new()),
            extra,
            pairing_until: Mutex::new(0.0),
            candidates: Mutex::new(HashMap::new()),
            linking: Mutex::new(None),
            seen: Mutex::new(HashMap::new()),
            private: Mutex::new(None),
            profiles: Mutex::new(Vec::new()),
            allow_public: AtomicBool::new(false),
        });
        for v in linked.iter().filter(|v| v.confirmado) {
            net.trust(v.clone());
        }
        net.dirty.store(false, Ordering::Release);
        net
    }

    pub fn public_hex(&self) -> String {
        hex(&self.public)
    }

    pub fn huella(&self) -> String {
        fingerprint(&self.public)
    }

    #[cfg(test)]
    fn secret_hex(&self) -> String {
        hex(self.secret.as_bytes())
    }

    /// The pair key of this computer and `pk` (the same on both sides), and its raw bytes.
    fn pair_key(&self, pk: &[u8; 32]) -> (ChaCha20Poly1305, [u8; 32]) {
        let shared = self.secret.diffie_hellman(&PublicKey::from(*pk));
        let (lo, hi) = if self.public <= *pk { (self.public, *pk) } else { (*pk, self.public) };
        let mut info = b"kaltsit-enlace-v2".to_vec();
        info.extend_from_slice(&lo);
        info.extend_from_slice(&hi);
        let mut okm = [0u8; 32];
        hkdf::Hkdf::<Sha256>::new(None, shared.as_bytes()).expand(&info, &mut okm).expect("32 bytes");
        (ChaCha20Poly1305::new(&okm.into()), okm)
    }

    fn trust(&self, v: Vinculo) {
        let Some(pk) = pk_of(&v.publica) else { return };
        let (key, bind) = self.pair_key(&pk);
        let mut t = lock(&self.trusted);
        t.retain(|x| x.pk != pk && x.v.id != v.id);
        t.push(Trusted { v, pk, key, bind, sessions: Vec::new(), tried: 0.0, addr: None });
        self.dirty.store(true, Ordering::Release);
    }

    fn untrust(&self, id: &str) {
        lock(&self.trusted).retain(|x| x.v.id != id);
        lock(&self.peers).remove(id);
        self.dirty.store(true, Ordering::Release);
    }

    /// The linked computers, to save in local.json.
    pub fn vinculos(&self) -> Vec<Vinculo> {
        lock(&self.trusted).iter().map(|t| t.v.clone()).collect()
    }

    /// Whether the trusted list changed since the last call.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn status(&self) -> MutexGuard<'_, Status> {
        lock(&self.status)
    }

    /// Linked computers heard from recently, by id.
    pub fn alive(&self) -> Vec<Peer> {
        let now = now();
        let mut v: Vec<Peer> = lock(&self.peers).values().filter(|p| now - p.seen < ALIVE_S).cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn peer(&self, id: &str) -> Option<Peer> {
        self.alive().into_iter().find(|p| p.id == id)
    }

    pub fn peer_by_name(&self, name: &str) -> Option<Peer> {
        self.alive().into_iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Age of the session with `id`, in seconds (None: no session).
    pub fn session_age(&self, id: &str) -> Option<f64> {
        lock(&self.trusted).iter().find(|t| t.v.id == id).and_then(|t| t.sessions.first()).map(|s| now() - s.created)
    }

    // ── sealing ──

    fn seal(key: &ChaCha20Poly1305, aad: &str, plain: &[u8]) -> Vec<u8> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(key.encrypt(&nonce, Payload { msg: plain, aad: aad.as_bytes() }).expect("encrypt"));
        out
    }

    /// Opens a sealed message, once: a nonce seen before is a replay.
    fn open(&self, key: &ChaCha20Poly1305, aad: &str, sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() < 12 + 16 {
            return None;
        }
        let nonce: [u8; 12] = sealed[..12].try_into().ok()?;
        let plain = key.decrypt(&nonce.into(), Payload { msg: &sealed[12..], aad: aad.as_bytes() }).ok()?;
        let now = now();
        let mut seen = lock(&self.seen);
        seen.retain(|_, t| now - *t < 2.0 * MAX_SKEW_S + 10.0);
        if seen.insert(nonce, now).is_some() {
            return None;
        }
        Some(plain)
    }

    fn fresh(t: f64) -> bool {
        (now() - t).abs() <= MAX_SKEW_S
    }

    // ── which networks ──

    /// Whether `ip` is on a network the link may use.
    pub fn allowed(&self, ip: IpAddr) -> bool {
        let IpAddr::V4(v4) = ip else { return ip.is_loopback() };
        if v4.is_loopback() || self.allow_public.load(Ordering::Acquire) {
            return true;
        }
        match &*lock(&self.private) {
            Some(nets) => nets.iter().any(|(n, p)| in_net(v4, *n, *p)),
            // Unknown (PowerShell failed): home ranges only.
            None => v4.is_private(),
        }
    }

    pub fn refresh_networks(&self) {
        *lock(&self.private) = private_networks();
        *lock(&self.profiles) = network_profiles();
    }

    /// Connected networks (name, Private / Public / DomainAuthenticated), as last asked.
    pub fn profiles(&self) -> Vec<(String, String)> {
        lock(&self.profiles).clone()
    }

    /// The private networks known now, as "192.168.1.34/24".
    pub fn private_list(&self) -> Option<Vec<String>> {
        lock(&self.private).as_ref().map(|v| v.iter().map(|(ip, p)| format!("{ip}/{p}")).collect())
    }

    /// Where beacons go: the broadcast address of each private network (and the extra ones).
    fn beacon_targets(&self) -> Vec<String> {
        let mut t: Vec<String> = match &*lock(&self.private) {
            Some(nets) => nets.iter().filter(|(ip, _)| !ip.is_loopback()).map(|(n, p)| format!("{}:{}", broadcast_of(*n, *p), self.disc)).collect(),
            None => vec![format!("255.255.255.255:{}", self.disc)],
        };
        if self.allow_public.load(Ordering::Acquire) {
            t.push(format!("255.255.255.255:{}", self.disc));
        }
        t.extend(self.extra.iter().cloned());
        t.dedup();
        t
    }

    // ── beacons ──

    fn beacon(&self) -> Beacon {
        Beacon { v: PROTOCOL, id: self.id.clone(), name: self.name.clone(), port: self.port, idle: crate::local::idle_seconds(), t: now(), status: self.status().clone() }
    }

    /// One beacon per linked computer (sealed with the session, or only where to reach this one
    /// while there is none), plus the clear announcement while linking.
    fn datagrams(&self) -> Vec<Vec<u8>> {
        let full = serde_json::to_vec(&self.beacon()).unwrap_or_default();
        let reach = serde_json::to_vec(&Reach { v: PROTOCOL, id: self.id.clone(), port: self.port, t: now() }).unwrap_or_default();
        let me = hex(&self.public[..8]);
        let mut out: Vec<Vec<u8>> = lock(&self.trusted)
            .iter()
            .map(|t| {
                let aad = format!("beacon|{}>{}", self.id, t.v.id);
                let d = match t.fresh_session() {
                    Some(s) => Datagram::Sealed { de: me.clone(), para: hex(&t.pk[..8]), s: Some(hex(&s.sid)), c: b64().encode(Self::seal(&s.key, &aad, &full)) },
                    None => Datagram::Sealed { de: me.clone(), para: hex(&t.pk[..8]), s: None, c: b64().encode(Self::seal(&t.key, &aad, &reach)) },
                };
                serde_json::to_vec(&d).unwrap_or_default()
            })
            .collect();
        if self.pairing_open() {
            let d = Datagram::Pairing { id: self.id.clone(), name: self.name.clone(), port: self.port, pk: self.public_hex() };
            out.push(serde_json::to_vec(&d).unwrap_or_default());
        }
        out
    }

    /// Reads one datagram: a linked computer's beacon, or a candidate while linking. Only a
    /// beacon sealed with a session makes a peer; one without says where to open the session.
    fn accept(&self, buf: &[u8], from: SocketAddr) -> Option<Peer> {
        if !self.allowed(from.ip()) {
            return None;
        }
        match serde_json::from_slice::<Datagram>(buf).ok()? {
            Datagram::Pairing { id, name, port, pk } => {
                let key = pk_of(&pk)?;
                if self.pairing_open() && id != self.id && !lock(&self.trusted).iter().any(|t| t.pk == key && t.v.confirmado) {
                    let c = Candidate { id: id.clone(), name, addr: format!("{}:{}", from.ip(), port), huella: fingerprint(&key), pk, seen: now() };
                    lock(&self.candidates).insert(id, c);
                }
                None
            }
            Datagram::Sealed { de, para, s, c } => {
                if para != hex(&self.public[..8]) {
                    return None;
                }
                let sealed = b64().decode(c).ok()?;
                let (key, id) = {
                    let t = lock(&self.trusted);
                    let peer = t.iter().find(|t| hex(&t.pk[..8]) == de)?;
                    let key = match &s {
                        Some(sid) => peer.session(&unhex(sid)?)?.key.clone(),
                        None => peer.key.clone(),
                    };
                    (key, peer.v.id.clone())
                };
                let plain = self.open(&key, &format!("beacon|{}>{}", id, self.id), &sealed)?;
                if s.is_none() {
                    let r: Reach = serde_json::from_slice(&plain).ok()?;
                    if r.v == PROTOCOL && r.id == id && Self::fresh(r.t) {
                        if let Some(t) = lock(&self.trusted).iter_mut().find(|t| t.v.id == id) {
                            t.addr = Some(format!("{}:{}", from.ip(), r.port));
                        }
                    }
                    return None;
                }
                let b: Beacon = serde_json::from_slice(&plain).ok()?;
                if b.v != PROTOCOL || b.id != id || !Self::fresh(b.t) {
                    return None;
                }
                let addr = format!("{}:{}", from.ip(), b.port);
                if let Some(t) = lock(&self.trusted).iter_mut().find(|t| t.v.id == id) {
                    t.addr = Some(addr.clone());
                }
                Some(Peer { id: b.id, name: b.name, addr, seen: now(), idle: b.idle, status: b.status })
            }
        }
    }

    /// Opens (or renews) the sessions that are missing or old, with every linked computer whose
    /// address is known. In the background: a computer that does not answer is retried later.
    fn keep_sessions(self: &Arc<Self>) {
        let due: Vec<(String, String)> = {
            let mut t = lock(&self.trusted);
            let now = now();
            t.iter_mut()
                .filter(|t| t.v.confirmado && now - t.tried > RETRY_S)
                .filter(|t| t.sessions.first().is_none_or(|s| now - s.created > SESSION_S * 0.9))
                .filter_map(|t| {
                    let addr = t.addr.clone()?;
                    t.tried = now;
                    Some((addr, t.v.id.clone()))
                })
                .collect()
        };
        for (addr, id) in due {
            let me = self.clone();
            std::thread::spawn(move || {
                let _ = me.handshake(&addr, &id);
            });
        }
    }

    /// Starts announcing this computer and listening for the others.
    pub fn start_discovery(self: &Arc<Self>) -> Result<(), String> {
        let rx = UdpSocket::bind(("0.0.0.0", self.disc)).map_err(|e| format!("puerto de descubrimiento {}: {e}", self.disc))?;
        self.refresh_networks();
        let me = self.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                if let Ok((n, from)) = rx.recv_from(&mut buf) {
                    if let Some(p) = me.accept(&buf[..n], from) {
                        lock(&me.peers).insert(p.id.clone(), p);
                    }
                }
            }
        });
        let me = self.clone();
        std::thread::spawn(move || {
            let Ok(tx) = UdpSocket::bind(("0.0.0.0", 0)) else { return };
            let _ = tx.set_broadcast(true);
            let mut round = 0u64;
            loop {
                // The laptop changes networks: which ones are private is asked again every 30 s.
                if round % 15 == 14 {
                    me.refresh_networks();
                }
                round += 1;
                me.expire_pairing();
                me.keep_sessions();
                for d in me.datagrams() {
                    for to in me.beacon_targets() {
                        let _ = tx.send_to(&d, to.as_str());
                    }
                }
                std::thread::sleep(Duration::from_secs(BEACON_S));
            }
        });
        Ok(())
    }

    // ── sessions ──

    /// Opens a new session with linked computer `to` at `addr`: one-use keys both ways, sealed
    /// with the pair key; the one-use secret is gone as soon as the key is derived.
    fn handshake(&self, addr: &str, to: &str) -> Result<(), String> {
        // Also with a computer accepted here but not yet confirmed: that is how linking ends.
        let pair = lock(&self.trusted).iter().find(|t| t.v.id == to).map(|t| (t.key.clone(), t.bind));
        let (key, bind) = pair.ok_or_else(|| format!("{addr}: equipo no vinculado"))?;
        let mine = EphemeralSecret::random_from_rng(OsRng);
        let mine_pub = PublicKey::from(&mine).to_bytes();
        let plain = serde_json::to_vec(&json!({ "t": now(), "e": hex(&mine_pub) })).map_err(|e| e.to_string())?;
        let sealed = Self::seal(&key, &format!("hola|{}>{}", self.id, to), &plain);
        let resp = Self::agent(5).post(&format!("http://{addr}/hola")).set("X-Kaltsit-De", &self.id).send_bytes(&sealed).map_err(|e| format!("{addr}: {e}"))?;
        let mut raw = Vec::new();
        resp.into_reader().take(4096).read_to_end(&mut raw).map_err(|e| e.to_string())?;
        let opened = self.open(&key, &format!("hola-ok|{}>{}|{}", to, self.id, hex(&sealed[..12])), &raw).ok_or_else(|| format!("{addr}: respuesta no auténtica"))?;
        let v: Value = serde_json::from_slice(&opened).map_err(|e| e.to_string())?;
        let theirs = v["e"].as_str().and_then(pk_of).ok_or("clave de sesión inválida")?;
        if !Self::fresh(v["t"].as_f64().unwrap_or(0.0)) {
            return Err(format!("{addr}: respuesta caducada"));
        }
        let shared = mine.diffie_hellman(&PublicKey::from(theirs));
        let session = session_from(shared.as_bytes(), &bind, &mine_pub, &theirs);
        self.add_session(to, session);
        Ok(())
    }

    fn add_session(&self, id: &str, s: Session) {
        if let Some(t) = lock(&self.trusted).iter_mut().find(|t| t.v.id == id) {
            t.sessions.insert(0, s);
            let now = now();
            t.sessions.retain(|s| now - s.created < 2.0 * SESSION_S);
            t.sessions.truncate(SESSIONS_KEPT);
        }
    }

    /// The session to send with: the newest one, opened now if there is none or it is old.
    fn session_for(&self, addr: &str, to: &str) -> Result<Session, String> {
        let fresh = lock(&self.trusted).iter().find(|t| t.v.id == to).and_then(|t| t.fresh_session().cloned());
        if let Some(s) = fresh {
            return Ok(s);
        }
        self.handshake(addr, to)?;
        lock(&self.trusted).iter().find(|t| t.v.id == to).and_then(|t| t.sessions.first().cloned()).ok_or_else(|| format!("{addr}: sin sesión"))
    }

    fn forget_sessions(&self, id: &str) {
        if let Some(t) = lock(&self.trusted).iter_mut().find(|t| t.v.id == id) {
            t.sessions.clear();
        }
    }

    // ── linking ──

    pub fn pairing_open(&self) -> bool {
        *lock(&self.pairing_until) > now()
    }

    /// Seconds "Vincular" stays open.
    pub fn pairing_left(&self) -> f64 {
        (*lock(&self.pairing_until) - now()).max(0.0)
    }

    /// Opens "Vincular" for two minutes.
    pub fn open_pairing(&self) {
        *lock(&self.pairing_until) = now() + PAIRING_S;
        *lock(&self.linking) = None;
    }

    pub fn close_pairing(&self) {
        *lock(&self.pairing_until) = 0.0;
        self.expire_pairing();
    }

    /// When "Vincular" closes, a linking nobody finished is forgotten.
    fn expire_pairing(&self) {
        if self.pairing_open() {
            lock(&self.candidates).retain(|_, c| now() - c.seen < ALIVE_S);
            return;
        }
        lock(&self.candidates).clear();
        let open = lock(&self.linking).as_ref().is_some_and(|l| !l.listo && !l.rechazado);
        if open {
            let l = lock(&self.linking).take().unwrap();
            let unconfirmed = lock(&self.trusted).iter().any(|t| t.v.id == l.id && !t.v.confirmado);
            if unconfirmed {
                self.untrust(&l.id);
            }
        }
    }

    pub fn candidates(&self) -> Vec<Candidate> {
        let mut v: Vec<Candidate> = lock(&self.candidates).values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn linking(&self) -> Option<Linking> {
        lock(&self.linking).clone()
    }

    /// This computer asks `candidate` to link. Both screens then show the same code.
    pub fn request_link(&self, candidate: &str) -> Result<Linking, String> {
        if !self.pairing_open() {
            return Err(crate::lang::tr("abre «Vincular» primero", "open «Link a computer» first"));
        }
        let c = lock(&self.candidates).get(candidate).cloned().ok_or_else(|| crate::lang::tr("ese equipo ya no está esperando a vincularse", "that computer is no longer waiting to link"))?;
        let pk = pk_of(&c.pk).ok_or_else(|| crate::lang::tr("clave pública inválida", "invalid public key"))?;
        let r = Self::agent(5)
            .post(&format!("http://{}/vincular", c.addr))
            .send_json(json!({ "id": self.id, "nombre": self.name, "puerto": self.port, "pk": self.public_hex() }))
            .map_err(|e| format!("{}: {e}", c.name))?;
        let v: Value = r.into_json().map_err(|e| e.to_string())?;
        if v["pk"].as_str() != Some(c.pk.as_str()) {
            return Err(crate::lang::tr("el otro equipo respondió con otra clave", "the other computer answered with a different key"));
        }
        let l = Linking { id: c.id, name: c.name, addr: c.addr, pk: c.pk, codigo: pairing_code(&self.public, &pk), entrante: false, acepte: false, listo: false, rechazado: false };
        *lock(&self.linking) = Some(l.clone());
        Ok(l)
    }

    /// The user compared the codes and answered. Accepting trusts the other key; when the other
    /// computer accepts too, a sealed `/vinculo-ok` gets through and both mark it confirmed.
    pub fn answer_link(&self, accept: bool) -> Result<(), String> {
        let mut l = lock(&self.linking).clone().ok_or_else(|| crate::lang::tr("no hay ninguna vinculación en curso", "there is no link in progress"))?;
        if !accept {
            *lock(&self.linking) = None;
            if lock(&self.trusted).iter().any(|t| t.v.id == l.id && !t.v.confirmado) {
                self.untrust(&l.id);
            }
            let _ = Self::agent(5).post(&format!("http://{}/vincular-no", l.addr)).send_json(json!({ "id": self.id }));
            return Ok(());
        }
        l.acepte = true;
        *lock(&self.linking) = Some(l.clone());
        self.trust(Vinculo { id: l.id.clone(), nombre: l.name.clone(), publica: l.pk.clone(), confirmado: false });
        // If the other one already accepted, this goes through and both are done.
        if self.send_sealed(&l.addr, &l.id, "POST", "/vinculo-ok", &json!({}), 5).is_ok() {
            self.confirm(&l.id);
        }
        Ok(())
    }

    fn confirm(&self, id: &str) {
        if let Some(t) = lock(&self.trusted).iter_mut().find(|t| t.v.id == id) {
            t.v.confirmado = true;
        }
        self.dirty.store(true, Ordering::Release);
        if let Some(l) = lock(&self.linking).as_mut().filter(|l| l.id == id) {
            l.listo = true;
        }
    }

    /// Unlinks `id` here and, if it answers, there too.
    pub fn unlink(&self, id: &str) {
        if let Some(p) = self.peer(id) {
            let _ = self.send_sealed(&p.addr, id, "POST", "/desvincular", &json!({}), 5);
        }
        self.untrust(id);
    }

    // ── HTTP between linked computers ──

    fn agent(timeout: u64) -> ureq::Agent {
        ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(3)).timeout(Duration::from_secs(timeout)).build()
    }

    fn id_at(&self, addr: &str) -> Option<String> {
        lock(&self.peers).values().find(|p| p.addr == addr).map(|p| p.id.clone())
    }

    /// One request to a linked computer, sealed with the session, and its sealed reply. If the
    /// other side lost the session (it restarted), a new one is opened and it is sent again.
    fn send_sealed(&self, addr: &str, to: &str, method: &str, path: &str, body: &Value, timeout: u64) -> Result<Value, String> {
        match self.send_once(addr, to, method, path, body, timeout) {
            Err(e) if e.contains("sesión desconocida") => {
                self.forget_sessions(to);
                self.send_once(addr, to, method, path, body, timeout)
            }
            r => r,
        }
    }

    fn send_once(&self, addr: &str, to: &str, method: &str, path: &str, body: &Value, timeout: u64) -> Result<Value, String> {
        let s = self.session_for(addr, to)?;
        let plain = serde_json::to_vec(&json!({ "t": now(), "m": method, "p": path, "b": body })).map_err(|e| e.to_string())?;
        let sealed = Self::seal(&s.key, &format!("req|{}>{}", self.id, to), &plain);
        let resp = Self::agent(timeout).post(&format!("http://{addr}/k")).set("X-Kaltsit-De", &self.id).set("X-Kaltsit-Sesion", &hex(&s.sid)).send_bytes(&sealed);
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let e = r.into_string().unwrap_or_default();
                return Err(format!("{addr}{path}: {code} {e}"));
            }
            Err(e) => return Err(format!("{addr}{path}: {e}")),
        };
        let mut raw = Vec::new();
        resp.into_reader().take(256 << 20).read_to_end(&mut raw).map_err(|e| e.to_string())?;
        // The reply is bound to this request's nonce: an old reply cannot be passed off as new.
        let aad = format!("resp|{}>{}|{}", to, self.id, hex(&sealed[..12]));
        let opened = self.open(&s.key, &aad, &raw).ok_or_else(|| format!("{addr}{path}: respuesta no auténtica"))?;
        let v: Value = serde_json::from_slice(&opened).map_err(|e| e.to_string())?;
        if !Self::fresh(v["t"].as_f64().unwrap_or(0.0)) {
            return Err(format!("{addr}{path}: respuesta caducada"));
        }
        match v["code"].as_u64().unwrap_or(500) {
            200 => Ok(v["b"].clone()),
            code => Err(format!("{addr}{path}: {code} {}", v["b"]["error"].as_str().unwrap_or(""))),
        }
    }

    pub fn post(&self, addr: &str, path: &str, body: &Value, timeout: u64) -> Result<Value, String> {
        let to = self.id_at(addr).ok_or_else(|| format!("{addr}: no es un equipo vinculado a la vista"))?;
        self.send_sealed(addr, &to, "POST", path, body, timeout)
    }

    pub fn get(&self, addr: &str, path: &str, timeout: u64) -> Result<Value, String> {
        let to = self.id_at(addr).ok_or_else(|| format!("{addr}: no es un equipo vinculado a la vista"))?;
        self.send_sealed(addr, &to, "GET", path, &Value::Null, timeout)
    }

    /// Serves the link. `handle(method, path, from_id, body)` gets only what a linked computer
    /// sealed with a session; sessions (`/hola`) and linking (`/vincular`) are answered here.
    pub fn serve<F>(self: &Arc<Self>, handle: F) -> Result<(), String>
    where
        F: Fn(&str, &str, &str, Value) -> Result<Value, (u16, String)> + Send + Sync + 'static,
    {
        let server = tiny_http::Server::http(("0.0.0.0", self.port)).map_err(|e| format!("puerto del enlace {}: {e}", self.port))?;
        let me = self.clone();
        let handle = Arc::new(handle);
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let me = me.clone();
                let handle = handle.clone();
                std::thread::spawn(move || me.respond(req, &*handle));
            }
        });
        Ok(())
    }

    fn respond<F>(&self, mut req: tiny_http::Request, handle: &F)
    where
        F: Fn(&str, &str, &str, Value) -> Result<Value, (u16, String)>,
    {
        let plain = |req: tiny_http::Request, code: u16, v: Value| {
            let r = tiny_http::Response::from_string(v.to_string()).with_status_code(code).with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(r);
        };
        let remote = req.remote_addr().copied();
        if !remote.is_some_and(|a| self.allowed(a.ip())) {
            return plain(req, 403, json!({ "error": "red no permitida" }));
        }
        let url = req.url().to_string();
        let header = |name: &str| req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string());
        let (from, sid) = (header("X-Kaltsit-De").unwrap_or_default(), header("X-Kaltsit-Sesion").and_then(|s| unhex(&s)));
        let mut raw = Vec::new();
        let _ = req.as_reader().take(256 << 20).read_to_end(&mut raw);
        match url.as_str() {
            "/hola" => {
                let pair = lock(&self.trusted).iter().find(|t| t.v.id == from).map(|t| (t.key.clone(), t.bind));
                let Some((key, bind)) = pair else {
                    return plain(req, 403, json!({ "error": "equipo no vinculado" }));
                };
                let Some(msg) = self.open(&key, &format!("hola|{}>{}", from, self.id), &raw) else {
                    return plain(req, 403, json!({ "error": "saludo no auténtico o repetido" }));
                };
                let v: Value = serde_json::from_slice(&msg).unwrap_or(Value::Null);
                let Some(theirs) = v["e"].as_str().and_then(pk_of).filter(|_| Self::fresh(v["t"].as_f64().unwrap_or(0.0))) else {
                    return plain(req, 403, json!({ "error": "saludo caducado" }));
                };
                let mine = EphemeralSecret::random_from_rng(OsRng);
                let mine_pub = PublicKey::from(&mine).to_bytes();
                let shared = mine.diffie_hellman(&PublicKey::from(theirs));
                self.add_session(&from, session_from(shared.as_bytes(), &bind, &theirs, &mine_pub));
                let reply = serde_json::to_vec(&json!({ "t": now(), "e": hex(&mine_pub) })).unwrap_or_default();
                let sealed = Self::seal(&key, &format!("hola-ok|{}>{}|{}", self.id, from, hex(&raw[..12])), &reply);
                let _ = req.respond(tiny_http::Response::from_data(sealed));
            }
            "/vincular" => {
                let v: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
                let (Some(id), Some(pk)) = (v["id"].as_str(), v["pk"].as_str().and_then(pk_of)) else {
                    return plain(req, 400, json!({ "error": "petición inválida" }));
                };
                let busy = lock(&self.linking).as_ref().is_some_and(|l| l.id != id && !l.rechazado);
                if !self.pairing_open() || busy {
                    return plain(req, 409, json!({ "error": "este equipo no está esperando a vincularse" }));
                }
                let ip = remote.map(|a| a.ip().to_string()).unwrap_or_default();
                let l = Linking {
                    id: id.to_string(),
                    name: v["nombre"].as_str().unwrap_or("otro equipo").to_string(),
                    addr: format!("{ip}:{}", v["puerto"].as_u64().unwrap_or(0)),
                    pk: hex(&pk),
                    codigo: pairing_code(&self.public, &pk),
                    entrante: true,
                    acepte: false,
                    listo: false,
                    rechazado: false,
                };
                *lock(&self.linking) = Some(l);
                plain(req, 200, json!({ "id": self.id, "nombre": self.name, "pk": self.public_hex() }))
            }
            "/vincular-no" => {
                let v: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
                let id = v["id"].as_str().unwrap_or("").to_string();
                // Only a linking still in progress can be called off this way.
                let called_off = {
                    let mut l = lock(&self.linking);
                    match l.as_mut().filter(|x| x.id == id && !x.listo) {
                        Some(x) => {
                            x.rechazado = true;
                            true
                        }
                        None => false,
                    }
                };
                if called_off && lock(&self.trusted).iter().any(|t| t.v.id == id && !t.v.confirmado) {
                    self.untrust(&id);
                }
                plain(req, 200, json!({ "ok": true }))
            }
            "/k" => {
                let session = sid.and_then(|sid| lock(&self.trusted).iter().find(|t| t.v.id == from).and_then(|t| t.session(&sid).cloned()));
                let Some(s) = session else {
                    let known = lock(&self.trusted).iter().any(|t| t.v.id == from);
                    return plain(req, 403, json!({ "error": if known { "sesión desconocida" } else { "equipo no vinculado" } }));
                };
                let Some(msg) = self.open(&s.key, &format!("req|{}>{}", from, self.id), &raw) else {
                    return plain(req, 403, json!({ "error": "mensaje no auténtico o repetido" }));
                };
                let v: Value = serde_json::from_slice(&msg).unwrap_or(Value::Null);
                if !Self::fresh(v["t"].as_f64().unwrap_or(0.0)) {
                    return plain(req, 403, json!({ "error": "mensaje caducado" }));
                }
                let (method, path, body) = (v["m"].as_str().unwrap_or("POST").to_string(), v["p"].as_str().unwrap_or("/").to_string(), v["b"].clone());
                let (code, out) = match path.as_str() {
                    "/vinculo-ok" => {
                        self.confirm(&from);
                        (200, json!({ "ok": true }))
                    }
                    "/desvincular" => {
                        self.untrust(&from);
                        (200, json!({ "ok": true }))
                    }
                    _ => match handle(&method, &path, &from, body) {
                        Ok(v) => (200, v),
                        Err((code, e)) => (code, json!({ "error": e })),
                    },
                };
                let reply = serde_json::to_vec(&json!({ "t": now(), "code": code, "b": out })).unwrap_or_default();
                let sealed = Self::seal(&s.key, &format!("resp|{}>{}|{}", self.id, from, hex(&raw[..12])), &reply);
                let r = tiny_http::Response::from_data(sealed).with_header(tiny_http::Header::from_bytes("Content-Type", "application/octet-stream").unwrap());
                let _ = req.respond(r);
            }
            _ => plain(req, 404, json!({ "error": "no existe" })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(id: &str, port: u16) -> Arc<Net> {
        Net::new(id.into(), id.to_uppercase(), port, port - 1, &new_identity(), &[], vec![])
    }

    /// Links two computers directly (what the dialog does after both users accept).
    fn link(a: &Net, b: &Net) {
        a.trust(Vinculo { id: b.id.clone(), nombre: b.name.clone(), publica: b.public_hex(), confirmado: true });
        b.trust(Vinculo { id: a.id.clone(), nombre: a.name.clone(), publica: a.public_hex(), confirmado: true });
    }

    /// `a` hears `b`'s beacons, from `ip`.
    fn see(a: &Net, b: &Net, ip: &str) {
        let from: SocketAddr = format!("{ip}:5555").parse().unwrap();
        for d in b.datagrams() {
            if let Some(p) = a.accept(&d, from) {
                lock(&a.peers).insert(p.id.clone(), p);
            }
        }
    }

    fn at(a: &Net, id: &str, addr: &str) {
        lock(&a.peers).insert(id.into(), Peer { id: id.into(), name: id.to_uppercase(), addr: addr.into(), seen: now(), idle: 0.0, status: Status::default() });
    }

    #[test]
    fn both_sides_derive_the_same_key_and_code() {
        let (a, b, c) = (net("a", 47001), net("b", 47003), net("c", 47005));
        let sealed = Net::seal(&a.pair_key(&b.public).0, "x", b"hola");
        assert_eq!(b.open(&b.pair_key(&a.public).0, "x", &sealed).unwrap(), b"hola");
        assert!(b.open(&b.pair_key(&a.public).0, "otra", &Net::seal(&a.pair_key(&b.public).0, "x", b"hola")).is_none(), "the context is authenticated");
        assert_eq!(pairing_code(&a.public, &b.public), pairing_code(&b.public, &a.public));
        assert_eq!(pairing_code(&a.public, &b.public).len(), 7);
        assert_ne!(pairing_code(&a.public, &b.public), pairing_code(&a.public, &c.public), "someone in the middle shows another code");
        assert_eq!(Net::new("x".into(), "X".into(), 1, 2, &a.secret_hex(), &[], vec![]).public, a.public, "the identity is kept");
    }

    #[test]
    fn only_linked_computers_see_each_other_and_only_with_a_session() {
        let (a, b, stranger) = (net("a", 47011), net("b", 47013), net("c", 47015));
        a.serve(|_, _, _, _| Ok(json!({}))).unwrap();
        b.status().brain = true;
        see(&a, &b, "127.0.0.1");
        assert!(a.alive().is_empty(), "not linked: nothing");
        link(&a, &b);
        // Without a session, b's beacon says only where to reach it.
        see(&a, &b, "127.0.0.1");
        assert!(a.alive().is_empty(), "no status before the session");
        assert_eq!(lock(&b.trusted)[0].sessions.len(), 0);
        // b opens the session (as its discovery thread would), then its beacons carry everything.
        b.handshake("127.0.0.1:47011", "a").unwrap();
        see(&a, &b, "127.0.0.1");
        let p = a.peer("b").expect("seen with a session");
        assert_eq!((p.name.as_str(), p.addr.as_str(), p.status.brain), ("B", "127.0.0.1:47013", true));
        see(&stranger, &b, "127.0.0.1");
        assert!(stranger.alive().is_empty(), "a third computer reads nothing");
        let d = b.datagrams().remove(0);
        let from: SocketAddr = "127.0.0.1:5555".parse().unwrap();
        assert!(a.accept(&d, from).is_some());
        assert!(a.accept(&d, from).is_none(), "the same beacon again is a replay");
        *lock(&a.private) = Some(vec![("192.168.1.0".parse().unwrap(), 24)]);
        assert!(a.accept(&b.datagrams().remove(0), "10.8.0.5:5555".parse().unwrap()).is_none(), "from a network that is not private");
    }

    #[test]
    fn requests_are_sealed_and_cannot_be_replayed_or_forged() {
        let a = net("a", 47021);
        a.serve(|m, path, from, v| Ok(json!({ "m": m, "path": path, "from": from, "echo": v }))).unwrap();
        let (b, stranger) = (net("b", 47023), net("c", 47025));
        link(&a, &b);
        at(&b, "a", "127.0.0.1:47021");
        let r = b.post("127.0.0.1:47021", "/oir", &json!({ "text": "hola" }), 5).unwrap();
        assert_eq!((r["path"].as_str(), r["from"].as_str(), r["echo"]["text"].as_str()), (Some("/oir"), Some("b"), Some("hola")));
        let g = b.get("127.0.0.1:47021", "/ojos?todo=1", 5).unwrap();
        assert_eq!((g["m"].as_str(), g["path"].as_str()), (Some("GET"), Some("/ojos?todo=1")), "the query arrives");
        let s = lock(&b.trusted)[0].sessions[0].clone();
        let plain = serde_json::to_vec(&json!({ "t": now(), "m": "POST", "p": "/oir", "b": {} })).unwrap();
        let send = |body: &[u8], from: &str, sid: &[u8]| ureq::post("http://127.0.0.1:47021/k").set("X-Kaltsit-De", from).set("X-Kaltsit-Sesion", &hex(sid)).send_bytes(body);
        let sealed = Net::seal(&s.key, "req|b>a", &plain);
        assert!(send(&sealed, "b", &s.sid).is_ok(), "the original goes through");
        assert!(matches!(send(&sealed, "b", &s.sid), Err(ureq::Error::Status(403, _))), "its copy does not");
        let mut tampered = Net::seal(&s.key, "req|b>a", &plain);
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(matches!(send(&tampered, "b", &s.sid), Err(ureq::Error::Status(403, _))), "tampered");
        let old = Net::seal(&s.key, "req|b>a", &serde_json::to_vec(&json!({ "t": now() - 600.0, "m": "POST", "p": "/oir", "b": {} })).unwrap());
        assert!(matches!(send(&old, "b", &s.sid), Err(ureq::Error::Status(403, _))), "stale");
        at(&stranger, "a", "127.0.0.1:47021");
        assert!(stranger.post("127.0.0.1:47021", "/oir", &json!({}), 5).is_err(), "not linked");
        assert!(matches!(send(&Net::seal(&stranger.pair_key(&a.public).0, "req|b>a", &plain), "b", &s.sid), Err(ureq::Error::Status(403, _))), "an impostor calling itself b");
    }

    #[test]
    fn a_stolen_identity_does_not_open_what_was_recorded() {
        let a = net("a", 47051);
        a.serve(|_, _, _, v| Ok(v)).unwrap();
        let b = net("b", 47053);
        link(&a, &b);
        at(&b, "a", "127.0.0.1:47051");
        b.post("127.0.0.1:47051", "/oir", &json!({ "text": "secreto" }), 5).unwrap();
        // Someone recorded a sealed request of this session…
        let s = lock(&b.trusted)[0].sessions[0].clone();
        let recorded = Net::seal(&s.key, "req|b>a", br#"{"p":"/oir","b":{"text":"secreto"}}"#);
        // …and later steals both identities: the pair key they give does not open it.
        let thief_a = Net::new("a".into(), "A".into(), 1, 2, &a.secret_hex(), &[], vec![]);
        let (pair_key, _) = thief_a.pair_key(&b.public);
        assert!(pair_key.decrypt(recorded[..12].into(), Payload { msg: &recorded[12..], aad: b"req|b>a" }).is_err(), "the session key is not in the identities");
        // The session key itself lives only in memory: a new run has none, and a new handshake
        // gives a different one.
        b.forget_sessions("a");
        b.post("127.0.0.1:47051", "/oir", &json!({}), 5).unwrap();
        let s2 = lock(&b.trusted)[0].sessions[0].clone();
        assert_ne!(s2.sid, s.sid, "every session is new");
        assert!(s2.key.decrypt(recorded[..12].into(), Payload { msg: &recorded[12..], aad: b"req|b>a" }).is_err());
    }

    #[test]
    fn a_restarted_computer_gets_a_new_session_by_itself() {
        let a = net("a", 47061);
        a.serve(|_, _, _, _| Ok(json!({ "ok": true }))).unwrap();
        let b = net("b", 47063);
        link(&a, &b);
        at(&b, "a", "127.0.0.1:47061");
        b.post("127.0.0.1:47061", "/x", &json!({}), 5).unwrap();
        let first = lock(&b.trusted)[0].sessions[0].sid;
        // a "restarts": it forgets every session. b's next request is refused once, then it opens
        // a new session and the request goes through.
        a.forget_sessions("b");
        assert!(b.post("127.0.0.1:47061", "/x", &json!({}), 5).is_ok());
        assert_ne!(lock(&b.trusted)[0].sessions[0].sid, first);
        // An old session is renewed after ten minutes.
        lock(&b.trusted)[0].sessions[0].created -= SESSION_S + 1.0;
        let before = lock(&b.trusted)[0].sessions[0].sid;
        b.post("127.0.0.1:47061", "/x", &json!({}), 5).unwrap();
        assert_ne!(lock(&b.trusted)[0].sessions[0].sid, before, "renewed");
        assert!(b.session_age("a").unwrap() < 5.0);
    }

    #[test]
    fn linking_needs_both_to_accept() {
        let (a, b) = (net("a", 47031), net("b", 47033));
        a.serve(|_, _, _, _| Ok(json!({ "ok": true }))).unwrap();
        b.serve(|_, _, _, _| Ok(json!({ "ok": true }))).unwrap();
        a.open_pairing();
        see(&a, &b, "127.0.0.1");
        assert!(a.candidates().is_empty(), "b is not offering itself");
        b.open_pairing();
        see(&a, &b, "127.0.0.1");
        let cand = a.candidates();
        assert_eq!((cand.len(), cand[0].addr.as_str()), (1, "127.0.0.1:47033"));
        let la = a.request_link("b").unwrap();
        let lb = b.linking().expect("b shows the request");
        assert!(lb.entrante && !la.entrante);
        assert_eq!(la.codigo, lb.codigo, "the same code on both screens");
        a.answer_link(true).unwrap();
        assert!(!a.linking().unwrap().listo, "not done until b accepts");
        at(&a, "b", "127.0.0.1:47033");
        assert!(a.post("127.0.0.1:47033", "/x", &json!({}), 5).is_err(), "b does not take a's messages yet");
        b.answer_link(true).unwrap();
        assert!(b.linking().unwrap().listo && a.linking().unwrap().listo, "both confirmed");
        assert!(a.vinculos().iter().all(|v| v.confirmado) && b.vinculos().iter().all(|v| v.confirmado));
        assert!(a.post("127.0.0.1:47033", "/x", &json!({}), 5).is_ok());
        at(&b, "a", "127.0.0.1:47031");
        a.unlink("b");
        assert!(a.vinculos().is_empty() && b.vinculos().is_empty(), "unlinking from one side unlinks both");
    }

    #[test]
    fn a_refusal_undoes_it() {
        let (a, b) = (net("a", 47041), net("b", 47043));
        a.serve(|_, _, _, _| Ok(json!({}))).unwrap();
        b.serve(|_, _, _, _| Ok(json!({}))).unwrap();
        a.open_pairing();
        b.open_pairing();
        see(&a, &b, "127.0.0.1");
        a.request_link("b").unwrap();
        a.answer_link(true).unwrap();
        b.answer_link(false).unwrap();
        assert!(a.linking().unwrap().rechazado);
        assert!(a.vinculos().is_empty() && b.vinculos().is_empty());
    }

    #[test]
    fn networks_and_broadcasts() {
        let lan: Ipv4Addr = "192.168.1.34".parse().unwrap();
        assert!(in_net("192.168.1.200".parse().unwrap(), lan, 24));
        assert!(!in_net("192.168.2.1".parse().unwrap(), lan, 24));
        assert_eq!(broadcast_of(lan, 24), "192.168.1.255".parse::<Ipv4Addr>().unwrap());
        assert_eq!(broadcast_of("10.5.0.7".parse().unwrap(), 16), "10.5.255.255".parse::<Ipv4Addr>().unwrap());
    }
}
