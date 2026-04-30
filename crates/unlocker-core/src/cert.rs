use anyhow::Result;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};
use std::path::{Path, PathBuf};

pub struct RootCa {
    pub cert: Certificate,
    pub key: KeyPair,
}

impl RootCa {
    pub fn cert_pem(&self) -> String {
        self.cert.pem()
    }
    pub fn key_pem(&self) -> String {
        self.key.serialize_pem()
    }
}

pub struct LeafCert {
    pub cert_pem: String,
    pub key_pem: String,
}

pub fn generate_root_ca() -> Result<RootCa> {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Xteink Unlocker Local CA");
    dn.push(DnType::OrganizationName, "CrossPoint Reader");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

    let key = KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    Ok(RootCa { cert, key })
}

pub fn mint_leaf(root: &RootCa, hostnames: &[&str]) -> Result<LeafCert> {
    mint_leaf_from_pem(&root.cert_pem(), &root.key_pem(), hostnames)
}

/// Mint a leaf cert against an issuer reconstructed from PEMs. Useful on the
/// helper side, where we receive PEM strings over the wire and don't need a
/// full `RootCa` value.
pub fn mint_leaf_from_pem(
    issuer_cert_pem: &str,
    issuer_key_pem: &str,
    hostnames: &[&str],
) -> Result<LeafCert> {
    let sans: Vec<String> = hostnames.iter().map(|s| (*s).to_string()).collect();
    let mut params = CertificateParams::new(sans)?;
    let mut dn = DistinguishedName::new();
    dn.push(
        DnType::CommonName,
        hostnames.first().copied().unwrap_or("localhost"),
    );
    params.distinguished_name = dn;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let issuer_key = KeyPair::from_pem(issuer_key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(issuer_cert_pem, issuer_key)?;
    let leaf_key = KeyPair::generate()?;
    let leaf = params.signed_by(&leaf_key, &issuer)?;
    Ok(LeafCert {
        cert_pem: leaf.pem(),
        key_pem: leaf_key.serialize_pem(),
    })
}

pub fn write_root_for_user(root: &RootCa, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let p = dir.join("xteink-unlocker-root.pem");
    std::fs::write(&p, root.cert_pem())?;
    Ok(p)
}
