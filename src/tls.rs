//! Local HTTPS so Android's Chrome can install the tablet app (it only offers
//! "Install app" on secure origins).
//!
//! On first start we create a small certificate authority and keep it in the
//! state directory. The tablet installs `ca.crt` once; after that every server
//! certificate signed here is trusted. The server certificate names the Pi's
//! hostname, `<hostname>.local` and current IP, and is re-issued whenever those
//! change, without touching the CA the tablet trusts.

use std::net::UdpSocket;
use std::path::Path;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};

pub struct Tls {
    /// Server certificate chain (leaf + CA) and key, PEM, for the HTTPS server.
    pub cert_chain_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    /// The CA certificate in DER form, served for installing on the tablet.
    pub ca_der: Vec<u8>,
    pub names: Vec<String>,
}

const CA_NAME: &str = "Piano LED local CA";

fn ca_params() -> CertificateParams {
    let mut p = CertificateParams::default();
    p.distinguished_name.push(DnType::CommonName, CA_NAME);
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    p
}

/// Names the server certificate must cover.
fn server_names() -> Vec<String> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        let h = h.trim();
        if !h.is_empty() {
            names.push(h.to_string());
            names.push(format!("{h}.local"));
        }
    }
    // The address we'd use to reach the LAN; connect() on UDP sends nothing.
    if let Some(ip) = UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("1.1.1.1:80").map(|_| s))
        .and_then(|s| s.local_addr())
        .ok()
        .map(|a| a.ip())
        .filter(|ip| !ip.is_unspecified() && !ip.is_loopback())
    {
        names.push(ip.to_string());
    }
    names
}

fn write_private(path: &Path, data: &str) -> std::io::Result<()> {
    std::fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn load_or_create(dir: &Path) -> Result<Tls, String> {
    fn err(what: &'static str) -> impl Fn(std::io::Error) -> String {
        move |e| format!("tls: {what}: {e}")
    }
    std::fs::create_dir_all(dir).map_err(err("create dir"))?;
    let (ca_key_path, ca_der_path, ca_pem_path) = (dir.join("ca.key"), dir.join("ca.crt"), dir.join("ca.pem"));

    let ca_key = if ca_key_path.exists() && ca_der_path.exists() && ca_pem_path.exists() {
        let pem = std::fs::read_to_string(&ca_key_path).map_err(err("read ca.key"))?;
        KeyPair::from_pem(&pem).map_err(|e| format!("tls: ca.key: {e}"))?
    } else {
        let key = KeyPair::generate().map_err(|e| format!("tls: {e}"))?;
        let ca = ca_params().self_signed(&key).map_err(|e| format!("tls: {e}"))?;
        write_private(&ca_key_path, &key.serialize_pem()).map_err(err("write ca.key"))?;
        std::fs::write(&ca_der_path, ca.der()).map_err(err("write ca.crt"))?;
        std::fs::write(&ca_pem_path, ca.pem()).map_err(err("write ca.pem"))?;
        eprintln!("tls: created a local CA in {}", dir.display());
        key
    };
    let ca_der = std::fs::read(&ca_der_path).map_err(err("read ca.crt"))?;
    let ca_pem = std::fs::read(&ca_pem_path).map_err(err("read ca.pem"))?;

    let names = server_names();
    let (crt_path, key_path, names_path) = (dir.join("server.crt"), dir.join("server.key"), dir.join("server.names"));
    let current = std::fs::read_to_string(&names_path).unwrap_or_default();
    if current != names.join("\n") || !crt_path.exists() || !key_path.exists() {
        let key = KeyPair::generate().map_err(|e| format!("tls: {e}"))?;
        let mut params = CertificateParams::new(names.clone()).map_err(|e| format!("tls: {e}"))?;
        params.distinguished_name.push(DnType::CommonName, names.last().cloned().unwrap_or_default());
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
        // The issuer only needs the CA's name and key; the tablet already has its certificate.
        let ca = ca_params();
        let issuer = Issuer::from_params(&ca, &ca_key);
        let cert = params.signed_by(&key, &issuer).map_err(|e| format!("tls: {e}"))?;
        write_private(&key_path, &key.serialize_pem()).map_err(err("write server.key"))?;
        std::fs::write(&crt_path, cert.pem()).map_err(err("write server.crt"))?;
        std::fs::write(&names_path, names.join("\n")).map_err(err("write server.names"))?;
        eprintln!("tls: server certificate for {}", names.join(", "));
    }
    let mut chain = std::fs::read(&crt_path).map_err(err("read server.crt"))?;
    chain.extend_from_slice(&ca_pem);
    Ok(Tls {
        cert_chain_pem: chain,
        key_pem: std::fs::read(&key_path).map_err(err("read server.key"))?,
        ca_der,
        names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_and_reuses_the_ca() {
        let dir = std::env::temp_dir().join(format!("plv-tls-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = load_or_create(&dir).unwrap();
        let b = load_or_create(&dir).unwrap();
        assert_eq!(a.ca_der, b.ca_der, "CA must survive restarts");
        assert!(String::from_utf8_lossy(&a.cert_chain_pem).matches("BEGIN CERTIFICATE").count() == 2);
        assert!(a.names.contains(&"localhost".to_string()));
        // Changed names -> new server cert, same CA.
        std::fs::write(dir.join("server.names"), "old-name").unwrap();
        let c = load_or_create(&dir).unwrap();
        assert_eq!(c.ca_der, a.ca_der);
        assert_ne!(c.cert_chain_pem, a.cert_chain_pem);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
