//! 本地一机一证自签证书管理、SHA-256 指纹固定校验、mTLS 双向认证与吊销黑名单 (P1 安全底座).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rcgen::generate_simple_self_signed;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_CERT_VALIDITY_SECS: u64 = 30 * 24 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CertTrustState {
    pub local_cert_sha256: String,
    pub local_cert_expires_at: u64,
    pub pinned_server_cert_sha256: Option<String>,
    pub backup_server_cert_sha256: Option<String>,
    pub revoked_fingerprints: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct LocalIdentity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub cert_sha256: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone)]
pub struct CertManager {
    device_id: String,
    certs_dir: PathBuf,
    identity: Arc<RwLock<LocalIdentity>>,
    trust_state: Arc<RwLock<CertTrustState>>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub fn normalize_fingerprint(fp: &str) -> String {
    fp.trim().replace(':', "").to_ascii_lowercase()
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ensure_private_dir_and_file_perms(dir: &Path, key_path: Option<&Path>) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        if let Some(kp) = key_path {
            if kp.exists() {
                let _ = fs::set_permissions(kp, fs::Permissions::from_mode(0o600));
            }
        }
    }
    Ok(())
}

impl CertManager {
    pub fn load_or_create(
        data_dir: &Path,
        device_id: &str,
        pinned_server_cert: Option<&str>,
    ) -> anyhow::Result<Self> {
        let certs_dir = data_dir.join("certs");
        ensure_private_dir_and_file_perms(&certs_dir, None)?;

        let cert_der_path = certs_dir.join("device.cert.der");
        let key_der_path = certs_dir.join("device.key.der");
        let state_path = certs_dir.join("trust_state.json");

        let mut trust_state: CertTrustState = if state_path.exists() {
            fs::read_to_string(&state_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            CertTrustState::default()
        };

        if let Some(pinned) = pinned_server_cert {
            let norm = normalize_fingerprint(pinned);
            if !norm.is_empty() {
                trust_state.pinned_server_cert_sha256 = Some(norm);
            }
        }

        let now = now_unix_secs();
        let identity = if cert_der_path.exists()
            && key_der_path.exists()
            && trust_state.local_cert_expires_at > now
        {
            let cert_der = fs::read(&cert_der_path)?;
            let key_der = fs::read(&key_der_path)?;
            let cert_sha256 = sha256_hex(&cert_der);
            LocalIdentity {
                cert_der,
                key_der,
                cert_sha256,
                expires_at: trust_state.local_cert_expires_at,
            }
        } else {
            let id = Self::generate_identity(device_id)?;
            fs::write(&cert_der_path, &id.cert_der)?;
            fs::write(&key_der_path, &id.key_der)?;
            ensure_private_dir_and_file_perms(&certs_dir, Some(&key_der_path))?;
            trust_state.local_cert_sha256 = id.cert_sha256.clone();
            trust_state.local_cert_expires_at = id.expires_at;
            fs::write(&state_path, serde_json::to_string_pretty(&trust_state)?)?;
            id
        };

        Ok(Self {
            device_id: device_id.to_string(),
            certs_dir,
            identity: Arc::new(RwLock::new(identity)),
            trust_state: Arc::new(RwLock::new(trust_state)),
        })
    }

    fn generate_identity(device_id: &str) -> anyhow::Result<LocalIdentity> {
        let mut sans = vec![
            device_id.to_string(),
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ];
        sans.dedup();
        let certified = generate_simple_self_signed(sans)?;
        let cert_der = certified.cert.der().to_vec();
        let key_der = certified.key_pair.serialize_der();
        let cert_sha256 = sha256_hex(&cert_der);
        let expires_at = now_unix_secs() + DEFAULT_CERT_VALIDITY_SECS;
        Ok(LocalIdentity {
            cert_der,
            key_der,
            cert_sha256,
            expires_at,
        })
    }

    fn persist_trust_state(&self, state: &CertTrustState) -> anyhow::Result<()> {
        let state_path = self.certs_dir.join("trust_state.json");
        fs::write(&state_path, serde_json::to_string_pretty(state)?)?;
        Ok(())
    }

    pub fn local_identity(&self) -> LocalIdentity {
        self.identity.read().expect("identity lock poisoned").clone()
    }

    pub fn trust_state(&self) -> CertTrustState {
        self.trust_state.read().expect("trust_state lock poisoned").clone()
    }

    pub fn pin_server_cert(&self, sha256: &str) -> anyhow::Result<()> {
        let norm = normalize_fingerprint(sha256);
        let mut guard = self.trust_state.write().expect("trust_state lock poisoned");
        if let Some(prev) = guard.pinned_server_cert_sha256.take() {
            if prev != norm {
                guard.backup_server_cert_sha256 = Some(prev);
            }
        }
        guard.pinned_server_cert_sha256 = Some(norm);
        self.persist_trust_state(&guard)
    }

    pub fn add_revoked_fingerprints(&self, fps: &[String]) -> anyhow::Result<()> {
        let mut guard = self.trust_state.write().expect("trust_state lock poisoned");
        for fp in fps {
            let norm = normalize_fingerprint(fp);
            if !norm.is_empty() {
                guard.revoked_fingerprints.insert(norm);
            }
        }
        self.persist_trust_state(&guard)
    }

    pub fn is_revoked(&self, fp: &str) -> bool {
        let norm = normalize_fingerprint(fp);
        let guard = self.trust_state.read().expect("trust_state lock poisoned");
        guard.revoked_fingerprints.contains(&norm)
    }

    /// 立即轮换本机自签证书并持久化新密钥对，旧指纹自动加入本地吊销集合以防旧会话复用.
    pub fn rotate_local_cert(&self) -> anyhow::Result<(String, String, u64)> {
        let new_id = Self::generate_identity(&self.device_id)?;
        let cert_der_path = self.certs_dir.join("device.cert.der");
        let key_der_path = self.certs_dir.join("device.key.der");
        fs::write(&cert_der_path, &new_id.cert_der)?;
        fs::write(&key_der_path, &new_id.key_der)?;
        ensure_private_dir_and_file_perms(&self.certs_dir, Some(&key_der_path))?;

        let old_fp = {
            let mut id_guard = self.identity.write().expect("identity lock poisoned");
            let old = id_guard.cert_sha256.clone();
            *id_guard = new_id.clone();
            old
        };

        {
            let mut ts_guard = self.trust_state.write().expect("trust_state lock poisoned");
            ts_guard.revoked_fingerprints.insert(old_fp.clone());
            ts_guard.local_cert_sha256 = new_id.cert_sha256.clone();
            ts_guard.local_cert_expires_at = new_id.expires_at;
            self.persist_trust_state(&ts_guard)?;
        }

        Ok((old_fp, new_id.cert_sha256, new_id.expires_at))
    }

    pub fn build_rotation_notice(
        &self,
        old_cert_sha256: &str,
        new_cert_sha256: &str,
        new_cert_expires_at: u64,
    ) -> mi_protocol::envelope::CertRotateNotice {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"MI-CERT-ROTATE-V1|");
        hasher.update(self.device_id.as_bytes());
        hasher.update(b"|");
        hasher.update(old_cert_sha256.as_bytes());
        hasher.update(b"|");
        hasher.update(new_cert_sha256.as_bytes());
        hasher.update(b"|");
        hasher.update(&new_cert_expires_at.to_be_bytes());
        let signature_hex = hasher.finalize().to_hex().to_string();
        mi_protocol::envelope::CertRotateNotice {
            device_id: self.device_id.clone(),
            old_cert_sha256: old_cert_sha256.to_string(),
            new_cert_sha256: new_cert_sha256.to_string(),
            new_cert_expires_at,
            signature_hex,
        }
    }

    pub fn build_cloud_tls_client_config(&self) -> anyhow::Result<Arc<ClientConfig>> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(CloudPinningVerifier {
            trust_state: self.trust_state.clone(),
            algs: provider.signature_verification_algorithms,
        });
        let cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        Ok(Arc::new(cfg))
    }

    pub fn build_lan_mtls_server_config(
        &self,
        allowed_peer_fps: Arc<RwLock<HashSet<String>>>,
    ) -> anyhow::Result<Arc<ServerConfig>> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let id = self.local_identity();
        let cert = CertificateDer::from(id.cert_der);
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(id.key_der));
        let client_verifier = Arc::new(LanPeerMtlsVerifier {
            trust_state: self.trust_state.clone(),
            allowed_peer_fps,
            algs: provider.signature_verification_algorithms,
        });
        let cfg = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(vec![cert], key)?;
        Ok(Arc::new(cfg))
    }

    pub fn build_lan_mtls_client_config(
        &self,
        allowed_peer_fps: Arc<RwLock<HashSet<String>>>,
    ) -> anyhow::Result<Arc<ClientConfig>> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let id = self.local_identity();
        let cert = CertificateDer::from(id.cert_der);
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(id.key_der));
        let server_verifier = Arc::new(LanPeerMtlsVerifier {
            trust_state: self.trust_state.clone(),
            allowed_peer_fps,
            algs: provider.signature_verification_algorithms,
        });
        let cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(server_verifier)
            .with_client_auth_cert(vec![cert], key)?;
        Ok(Arc::new(cfg))
    }
}

#[derive(Debug)]
struct CloudPinningVerifier {
    trust_state: Arc<RwLock<CertTrustState>>,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for CloudPinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual_fp = sha256_hex(end_entity.as_ref());
        let guard = self.trust_state.read().expect("trust_state lock poisoned");
        if guard.revoked_fingerprints.contains(&actual_fp) {
            return Err(rustls::Error::General(format!(
                "CertificateRevoked: {}",
                actual_fp
            )));
        }
        if let Some(ref primary) = guard.pinned_server_cert_sha256 {
            let backup_match = guard
                .backup_server_cert_sha256
                .as_deref()
                .map(|b| b == actual_fp)
                .unwrap_or(false);
            if primary != &actual_fp && !backup_match {
                return Err(rustls::Error::General(format!(
                    "TlsFingerprintMismatch: expected {}, actual {}",
                    primary, actual_fp
                )));
            }
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

#[derive(Debug)]
struct LanPeerMtlsVerifier {
    trust_state: Arc<RwLock<CertTrustState>>,
    allowed_peer_fps: Arc<RwLock<HashSet<String>>>,
    algs: WebPkiSupportedAlgorithms,
}

impl LanPeerMtlsVerifier {
    fn check_peer_cert(&self, cert: &CertificateDer<'_>) -> Result<(), rustls::Error> {
        let actual_fp = sha256_hex(cert.as_ref());
        {
            let guard = self.trust_state.read().expect("trust_state lock poisoned");
            if guard.revoked_fingerprints.contains(&actual_fp) {
                return Err(rustls::Error::General(format!(
                    "CertificateRevoked: {}",
                    actual_fp
                )));
            }
        }
        let allowed = self.allowed_peer_fps.read().expect("allowed_peer_fps lock poisoned");
        if !allowed.is_empty() && !allowed.contains(&actual_fp) {
            return Err(rustls::Error::General(format!(
                "LanPeerFingerprintUntrusted: {}",
                actual_fp
            )));
        }
        Ok(())
    }
}

impl ServerCertVerifier for LanPeerMtlsVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.check_peer_cert(end_entity)?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

impl ClientCertVerifier for LanPeerMtlsVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.check_peer_cert(end_entity)?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_protocol::envelope::compute_sas_pairing_code;

    #[test]
    fn test_local_cert_generation_and_rotation_with_revocation() {
        let dir = std::env::temp_dir().join(format!("mi_cert_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mgr = CertManager::load_or_create(&dir, "node_a", Some("aa:bb:cc:11")).unwrap();
        let id1 = mgr.local_identity();
        assert_eq!(id1.cert_sha256.len(), 64);
        assert_eq!(
            mgr.trust_state().pinned_server_cert_sha256.as_deref(),
            Some("aabbcc11")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let key_mode = fs::metadata(dir.join("certs/device.key.der"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(key_mode, 0o600);
        }

        let (old_fp, new_fp, _exp) = mgr.rotate_local_cert().unwrap();
        assert_eq!(old_fp, id1.cert_sha256);
        assert_ne!(old_fp, new_fp);
        assert!(mgr.is_revoked(&old_fp));
        assert!(!mgr.is_revoked(&new_fp));

        let sas1 = compute_sas_pairing_code("aabbcc11", &new_fp, "nonce123");
        let sas2 = compute_sas_pairing_code("AA:BB:CC:11".replace(':', "").as_str(), &new_fp, "nonce123");
        assert_eq!(sas1.len(), 6);
        assert_eq!(sas1, sas2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cloud_pinning_verifier_rejects_mismatch_and_revoked() {
        let dir = std::env::temp_dir().join(format!("mi_pin_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mgr = CertManager::load_or_create(&dir, "node_b", None).unwrap();
        let fake_cert = rcgen::generate_simple_self_signed(vec!["cloud.local".to_string()]).unwrap();
        let fake_der = CertificateDer::from(fake_cert.cert.der().to_vec());
        let fake_fp = sha256_hex(fake_der.as_ref());

        // Pin a different fingerprint -> must fail
        mgr.pin_server_cert("0000111122223333").unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = CloudPinningVerifier {
            trust_state: mgr.trust_state.clone(),
            algs: provider.signature_verification_algorithms,
        };
        let sname = ServerName::try_from("cloud.local").unwrap();
        let err = verifier
            .verify_server_cert(&fake_der, &[], &sname, &[], UnixTime::now())
            .unwrap_err();
        assert!(err.to_string().contains("TlsFingerprintMismatch"));

        // Pin matching fingerprint -> passes
        mgr.pin_server_cert(&fake_fp).unwrap();
        assert!(verifier
            .verify_server_cert(&fake_der, &[], &sname, &[], UnixTime::now())
            .is_ok());

        // Revoke fingerprint -> must fail with CertificateRevoked
        mgr.add_revoked_fingerprints(&[fake_fp]).unwrap();
        let err_rev = verifier
            .verify_server_cert(&fake_der, &[], &sname, &[], UnixTime::now())
            .unwrap_err();
        assert!(err_rev.to_string().contains("CertificateRevoked"));

        let _ = fs::remove_dir_all(&dir);
    }
}
