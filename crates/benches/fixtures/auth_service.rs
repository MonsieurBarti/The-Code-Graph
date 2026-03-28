use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AuthToken {
    pub access_token: String,
    pub expires_in: u64,
}

#[derive(Debug)]
pub struct AuthService {
    secret: Vec<u8>,
    token_ttl: Duration,
    revoked: HashMap<String, u64>,
}

impl AuthService {
    pub fn new(secret: &[u8], token_ttl: Duration) -> Self {
        Self {
            secret: secret.to_vec(),
            token_ttl,
            revoked: HashMap::new(),
        }
    }

    pub fn issue_token(&self, subject: &str, roles: &[&str]) -> AuthToken {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let exp = now + self.token_ttl.as_secs();
        let payload = format!("{}.{}.{}", subject, exp, roles.join(","));
        let signature = self.sign(&payload);
        let token = format!("{}.{}", base64_encode(&payload), base64_encode(&signature));
        AuthToken { access_token: token, expires_in: self.token_ttl.as_secs() }
    }

    pub fn validate_token(&self, token: &str) -> Option<Claims> {
        let parts: Vec<&str> = token.splitn(2, '.').collect();
        if parts.len() != 2 { return None; }
        let payload = base64_decode(parts[0])?;
        let sig = base64_decode(parts[1])?;
        if self.sign(&payload) != sig { return None; }
        let segments: Vec<&str> = payload.splitn(3, '.').collect();
        if segments.len() != 3 { return None; }
        let exp: u64 = segments[1].parse().ok()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if exp < now { return None; }
        let roles = segments[2].split(',').map(String::from).collect();
        Some(Claims { sub: segments[0].to_string(), exp, roles })
    }

    pub fn revoke(&mut self, token: &str) {
        let id = Uuid::new_v4().to_string();
        let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        self.revoked.insert(token.to_string(), exp);
    }

    fn sign(&self, data: &str) -> Vec<u8> {
        let mut mac = HmacSha256::new_from_slice(&self.secret).unwrap();
        mac.update(data.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }
}

fn base64_encode(s: &str) -> String { base64::encode(s.as_bytes()) }
fn base64_decode(s: &str) -> Option<String> {
    base64::decode(s).ok().and_then(|b| String::from_utf8(b).ok())
}
