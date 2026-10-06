//! Passkeys: an optional second way into an account, next to its password.
//!
//! - A signed-in user registers a passkey: a discoverable credential, user
//!   verification required, no attestation. The authenticator stores the
//!   user's id as its user handle.
//! - Sign-in asks for any passkey of this site (usernameless, autofill-ready):
//!   the authenticator says whose it is, the server checks the signature.
//! - Passwords stay, so any passkey (the last one too) can be removed.
//!
//! webauthn-rs checks challenge, origin, RP ID, user verification, signature
//! and counter. Ceremony states stay here, in memory, single-use, five
//! minutes at most: webauthn-rs insists they never reach the client. Same
//! rules as ariane/maison (`docs/dependances/passkeys.md` there).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use iris_core::ids::UserId;
use iris_db::SqlitePool;
use iris_db::passkeys::PasskeyRow;
use url::Url;
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, CredentialID, DiscoverableAuthentication, DiscoverableKey, Passkey,
    PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse, Webauthn, WebauthnBuilder,
};

use crate::error::{ApiError, ApiResult};

/// How long a ceremony may take, from options to answer; also the browser's
/// timeout.
pub const CEREMONY_TTL: Duration = Duration::from_mins(5);
/// Ceremonies in flight at most; the oldest goes first.
const CEREMONY_CAP: usize = 1_000;
/// Ceremonies in flight per address at most; its oldest goes first.
const CEREMONIES_PER_ADDRESS: usize = 20;

/// Values kept server-side for a short while, each taken at most once.
struct Ceremonies<T> {
    ttl: Duration,
    cap: usize,
    per_address: usize,
    map: Mutex<HashMap<String, (Instant, IpAddr, T)>>,
}

impl<T> Ceremonies<T> {
    fn new(ttl: Duration, cap: usize, per_address: usize) -> Self {
        Self {
            ttl,
            cap,
            per_address,
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Keep `value`, started from `ip`; returns the id to send back with the
    /// answer.
    fn put(&self, ip: IpAddr, value: T) -> String {
        let id = Uuid::new_v4().simple().to_string();
        let from = address_key(ip);
        let mut map = self.map.lock().unwrap_or_else(PoisonError::into_inner);
        map.retain(|_, (t, _, _)| t.elapsed() < self.ttl);
        let oldest = |map: &HashMap<String, (Instant, IpAddr, T)>, only: Option<IpAddr>| {
            map.iter()
                .filter(|(_, (_, a, _))| only.is_none_or(|o| *a == o))
                .min_by_key(|(_, (t, _, _))| *t)
                .map(|(k, _)| k.clone())
        };
        if map.values().filter(|(_, a, _)| *a == from).count() >= self.per_address
            && let Some(k) = oldest(&map, Some(from))
        {
            map.remove(&k);
        }
        if map.len() >= self.cap
            && let Some(k) = oldest(&map, None)
        {
            map.remove(&k);
        }
        map.insert(id.clone(), (Instant::now(), from, value));
        id
    }

    /// The value, once, if it is still fresh.
    fn take(&self, id: &str) -> Option<T> {
        let (t, _, v) = self
            .map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id)?;
        (t.elapsed() < self.ttl).then_some(v)
    }
}

/// What counts as one address: an IPv4 address, or an IPv6 /64 (one
/// household's prefix).
fn address_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from_bits(v6.to_bits() & (u128::MAX << 64))),
        v4 @ IpAddr::V4(_) => v4,
    }
}

enum Ceremony {
    Register {
        state: PasskeyRegistration,
        user_id: Uuid,
    },
    Login {
        state: DiscoverableAuthentication,
    },
}

pub struct Passkeys {
    webauthn: Webauthn,
    ceremonies: Ceremonies<Ceremony>,
}

/// The relying party for `public_url`: its host is the RP ID, its origin the
/// only one accepted. Passkeys are tied to the RP ID for good, so it must be
/// a stable name: no IP address, https (or `http://localhost` for dev), no
/// path.
fn relying_party(public_url: &str) -> anyhow::Result<(String, Url)> {
    let url = Url::parse(public_url.trim())?;
    let host = match url.host() {
        Some(url::Host::Domain(d)) => d.to_owned(),
        Some(_) => anyhow::bail!("public_url is an IP address; passkeys need a domain name"),
        None => anyhow::bail!("public_url has no host"),
    };
    let secure = url.scheme() == "https" || (url.scheme() == "http" && host == "localhost");
    if !secure {
        anyhow::bail!("passkeys need https (or http://localhost)");
    }
    if url.path() != "/" || url.query().is_some() {
        anyhow::bail!("public_url must be an origin, without a path");
    }
    Ok((host, url))
}

impl Passkeys {
    pub fn new(public_url: &str) -> anyhow::Result<Self> {
        let (rp_id, origin) = relying_party(public_url)?;
        let webauthn = WebauthnBuilder::new(&rp_id, &origin)?
            .rp_name("Iris")
            .timeout(CEREMONY_TTL)
            .build()?;
        tracing::info!(%rp_id, %origin, "passkeys ready");
        Ok(Self {
            webauthn,
            ceremonies: Ceremonies::new(CEREMONY_TTL, CEREMONY_CAP, CEREMONIES_PER_ADDRESS),
        })
    }

    /// Options for `navigator.credentials.create()`, and the ceremony id to
    /// send back with the answer.
    pub async fn registration_options(
        &self,
        db: &SqlitePool,
        ip: IpAddr,
        user: &iris_core::user::User,
    ) -> ApiResult<(String, CreationChallengeResponse)> {
        let user_id: Uuid = user.id.into();
        let exclude: Vec<CredentialID> = iris_db::passkeys::credential_ids_of(db, user.id)
            .await?
            .iter()
            .filter_map(|c| credential_id(c))
            .collect();
        let (mut options, state) = self
            .webauthn
            .start_passkey_registration(user_id, &user.email, &user.display_name, Some(exclude))
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("passkey registration start: {e}")))?;
        // A discoverable credential, so that sign-in needs no name
        // (webauthn-rs asks for "discouraged"; its registration check does
        // not depend on it).
        if let Some(sel) = options.public_key.authenticator_selection.as_mut() {
            sel.resident_key = Some(webauthn_rs_proto::ResidentKeyRequirement::Required);
            sel.require_resident_key = true;
        }
        let ceremony = self
            .ceremonies
            .put(ip, Ceremony::Register { state, user_id });
        Ok((ceremony, options))
    }

    /// Check the authenticator's answer and store the new passkey.
    pub async fn finish_registration(
        &self,
        db: &SqlitePool,
        user_id: UserId,
        ceremony: &str,
        credential: &RegisterPublicKeyCredential,
        name: &str,
    ) -> ApiResult<PasskeyRow> {
        let Some(Ceremony::Register {
            state,
            user_id: started_by,
        }) = self.ceremonies.take(ceremony)
        else {
            return Err(expired());
        };
        if started_by != Uuid::from(user_id) {
            return Err(ApiError::Forbidden);
        }
        let pk = self
            .webauthn
            .finish_passkey_registration(credential, &state)
            .map_err(|e| {
                tracing::warn!(error = %e, "passkey registration refused");
                ApiError::Unauthorized
            })?;
        let stored = Stored::of(&pk)?;
        let row = iris_db::passkeys::insert(
            db,
            &iris_db::passkeys::NewPasskey {
                user_id,
                credential_id: &credential_text(pk.cred_id())?,
                passkey_json: &stored.json,
                name,
                backup_eligible: stored.backup_eligible,
                backed_up: stored.backed_up,
            },
        )
        .await?;
        tracing::info!(user = %Uuid::from(user_id), "passkey added");
        Ok(row)
    }

    /// Options for `navigator.credentials.get()`: any passkey of this site.
    /// `conditional` keeps `mediation: "conditional"` for autofill; a button
    /// asks modally.
    pub fn login_options(
        &self,
        ip: IpAddr,
        conditional: bool,
    ) -> ApiResult<(String, RequestChallengeResponse)> {
        let (mut options, state) = self
            .webauthn
            .start_discoverable_authentication()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("passkey sign-in start: {e}")))?;
        if !conditional {
            options.mediation = None;
        }
        let ceremony = self.ceremonies.put(ip, Ceremony::Login { state });
        Ok((ceremony, options))
    }

    /// Check a sign-in and name whose passkey it was.
    pub async fn finish_login(
        &self,
        db: &SqlitePool,
        ceremony: &str,
        credential: &PublicKeyCredential,
    ) -> ApiResult<UserId> {
        let Some(Ceremony::Login { state }) = self.ceremonies.take(ceremony) else {
            return Err(expired());
        };
        let (handle, raw_id) = self
            .webauthn
            .identify_discoverable_authentication(credential)
            .map_err(|_| ApiError::Unauthorized)?;
        let cred_id = credential_text(&CredentialID::from(raw_id.to_vec()))?;
        let row = iris_db::passkeys::find_by_credential(db, &cred_id)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        if row.user_id != handle {
            tracing::warn!(user = %row.user_id, "passkey sign-in refused: not this user's");
            return Err(ApiError::Unauthorized);
        }
        let mut pk: Passkey = serde_json::from_str(&row.passkey_json)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("stored passkey: {e}")))?;
        let result = self
            .webauthn
            .finish_discoverable_authentication(credential, state, &[DiscoverableKey::from(&pk)])
            .map_err(|e| {
                tracing::warn!(error = %e, user = %row.user_id, "passkey sign-in refused");
                ApiError::Unauthorized
            })?;
        if !result.user_verified() {
            return Err(ApiError::Unauthorized);
        }
        let changed = pk.update_credential(&result) == Some(true);
        let updated = changed.then(|| Stored::of(&pk)).transpose()?;
        iris_db::passkeys::mark_used(
            db,
            row.id,
            updated
                .as_ref()
                .map(|s| (s.json.as_str(), s.backup_eligible, s.backed_up)),
        )
        .await?;
        tracing::info!(user = %row.user_id, "signed in with a passkey");
        Ok(UserId::from(row.user_id))
    }
}

fn expired() -> ApiError {
    ApiError::BadRequest("passkey request expired, start again".into())
}

/// A credential id as text: base64url, as webauthn-rs writes it.
fn credential_text(id: &CredentialID) -> ApiResult<String> {
    match serde_json::to_value(id) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        _ => Err(ApiError::Internal(anyhow::anyhow!(
            "credential id not a string"
        ))),
    }
}

fn credential_id(text: &str) -> Option<CredentialID> {
    serde_json::from_value(serde_json::Value::String(text.into())).ok()
}

/// The serialised credential and the backup flags shown in the list.
struct Stored {
    json: String,
    backup_eligible: bool,
    backed_up: bool,
}

impl Stored {
    fn of(pk: &Passkey) -> ApiResult<Self> {
        let v = serde_json::to_value(pk)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialise passkey: {e}")))?;
        let cred = &v["cred"];
        Ok(Self {
            backup_eligible: cred["backup_eligible"].as_bool().unwrap_or(false),
            backed_up: cred["backup_state"].as_bool().unwrap_or(false),
            json: v.to_string(),
        })
    }
}

/// A passkey's first name, from the device it was made on; the user renames
/// it. Empty when unknown: the client shows its own word.
pub fn device_label(user_agent: &str) -> &'static str {
    const DEVICES: &[(&str, &str)] = &[
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("CrOS", "ChromeOS"),
        ("Macintosh", "Mac"),
        ("Windows", "Windows"),
        ("Linux", "Linux"),
    ];
    DEVICES
        .iter()
        .find(|(needle, _)| user_agent.contains(needle))
        .map_or("", |(_, label)| label)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::time::Duration;

    use passkey::authenticator::{Authenticator, UiHint, UserCheck, UserValidationMethod};
    use passkey::client::{Client, DefaultClientData};
    use passkey::crypto::rust_crypto::RustCryptoBackend;
    use passkey::types::Passkey as SoftPasskey;
    use passkey::types::ctap2::{Aaguid, Ctap2Error};
    use passkey::types::webauthn::{CredentialCreationOptions, CredentialRequestOptions};
    use url::Url;

    use super::{Ceremonies, Passkeys, address_key, device_label, relying_party};

    const ORIGIN: &str = "https://iris.example.com";

    /// A person at the device: always there, always verified.
    struct Verified;

    #[async_trait::async_trait]
    impl UserValidationMethod for Verified {
        type PasskeyItem = SoftPasskey;

        async fn check_user<'a>(
            &self,
            _hint: UiHint<'a, SoftPasskey>,
            presence: bool,
            verification: bool,
        ) -> Result<UserCheck, Ctap2Error> {
            Ok(UserCheck {
                presence,
                verification,
            })
        }

        fn is_verification_enabled(&self) -> Option<bool> {
            Some(true)
        }

        fn is_presence_enabled(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn register_then_sign_in_with_the_same_device() {
        let db = iris_db::test_support::migrated_pool().await;
        let user = iris_db::users::create(
            &db,
            iris_db::users::NewUser {
                email: "leonard@example.com".into(),
                password_hash: "x".into(),
                is_admin: false,
            },
        )
        .await
        .unwrap();
        let passkeys = Passkeys::new(ORIGIN).unwrap();
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let mut device = Client::new(Authenticator::new(
            Aaguid::new_empty(),
            None::<SoftPasskey>,
            Verified,
            RustCryptoBackend,
        ));
        let origin = Url::parse(ORIGIN).unwrap();

        let (ceremony, options) = passkeys.registration_options(&db, ip, &user).await.unwrap();
        let request: CredentialCreationOptions =
            serde_json::from_value(serde_json::to_value(&options).unwrap()).unwrap();
        let created = device
            .register(origin.clone(), request, DefaultClientData)
            .await
            .unwrap();
        let credential = serde_json::from_value(serde_json::to_value(created).unwrap()).unwrap();
        let row = passkeys
            .finish_registration(&db, user.id, &ceremony, &credential, "Mac")
            .await
            .unwrap();
        assert_eq!(row.name, "Mac");
        // A ceremony answers once.
        assert!(
            passkeys
                .finish_registration(&db, user.id, &ceremony, &credential, "Mac")
                .await
                .is_err()
        );

        let (ceremony, options) = passkeys.login_options(ip, false).unwrap();
        let request: CredentialRequestOptions =
            serde_json::from_value(serde_json::to_value(&options).unwrap()).unwrap();
        let got = device
            .authenticate(origin, request, DefaultClientData)
            .await
            .unwrap();
        let assertion = serde_json::from_value(serde_json::to_value(got).unwrap()).unwrap();
        let signed_in = passkeys
            .finish_login(&db, &ceremony, &assertion)
            .await
            .unwrap();
        assert_eq!(signed_in, user.id);
        let rows = iris_db::passkeys::list_for_user(&db, user.id)
            .await
            .unwrap();
        assert!(rows[0].last_used_at.is_some());
    }

    #[test]
    fn ceremonies_are_single_use_and_bounded_per_address() {
        let ip: IpAddr = "100.64.0.1".parse().unwrap();
        let c = Ceremonies::new(Duration::from_secs(60), 100, 2);
        let a = c.put(ip, 1);
        assert_eq!(c.take(&a), Some(1));
        assert_eq!(c.take(&a), None, "single use");
        let other = c.put("100.64.0.9".parse().unwrap(), 0);
        let ids: Vec<_> = (1..=3).map(|n| c.put(ip, n)).collect();
        assert_eq!(c.take(&ids[0]), None, "the address's oldest went first");
        assert_eq!(c.take(&ids[2]), Some(3));
        assert_eq!(c.take(&other), Some(0));
        let expired = Ceremonies::new(Duration::ZERO, 3, 3);
        let b = expired.put(ip, 2);
        assert_eq!(expired.take(&b), None);
    }

    #[test]
    fn an_ipv6_household_is_one_address() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:bbbb::2".parse().unwrap();
        assert_eq!(address_key(a), address_key(b));
        assert_eq!(
            address_key("::ffff:192.0.2.1".parse().unwrap()),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn relying_party_from_public_url() {
        let (id, origin) = relying_party("https://iris.example.com").unwrap();
        assert_eq!(id, "iris.example.com");
        assert_eq!(origin.as_str(), "https://iris.example.com/");
        assert!(relying_party("http://localhost:8080").is_ok());
        assert!(
            relying_party("https://192.0.2.10").is_err(),
            "no IP address"
        );
        assert!(
            relying_party("http://iris.example.com").is_err(),
            "no plain http"
        );
        assert!(
            relying_party("https://iris.example.com/app").is_err(),
            "no path"
        );
    }

    #[test]
    fn devices_name_passkeys() {
        assert_eq!(
            device_label("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"),
            "Mac"
        );
        assert_eq!(
            device_label("Mozilla/5.0 (Linux; Android 16; Pixel 10)"),
            "Android"
        );
        assert_eq!(device_label("curl/8"), "");
    }
}
