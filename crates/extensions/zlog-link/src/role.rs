use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    Off,
    #[default]
    Client,
    Server,
}

impl Role {
    pub const CHOICES: [Role; 3] = [Role::Off, Role::Client, Role::Server];

    pub fn key(self) -> &'static str {
        match self {
            Role::Off => "off",
            Role::Client => "client",
            Role::Server => "server",
        }
    }

    pub fn parse(text: &str) -> Role {
        let t = text.trim().to_ascii_lowercase();
        match (t.contains("client"), t.contains("server")) {
            (false, true) => Role::Server,
            (false, false) if t == "off" => Role::Off,
            _ => Role::default(),
        }
    }

    fn code(self) -> u8 {
        match self {
            Role::Off => 0,
            Role::Client => 1,
            Role::Server => 2,
        }
    }
}

static ACTIVE: AtomicU8 = AtomicU8::new(0);

pub fn active() -> Role {
    match ACTIVE.load(Ordering::SeqCst) {
        1 => Role::Client,
        2 => Role::Server,
        _ => Role::Off,
    }
}

pub fn claim(role: Role) -> Result<(), Role> {
    let want = role.code();
    if want == 0 {
        return Err(Role::Off);
    }
    match ACTIVE.compare_exchange(0, want, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => Ok(()),
        Err(now) if now == want => Ok(()),
        Err(_) => Err(active()),
    }
}

pub fn release(role: Role) {
    let _ = ACTIVE.compare_exchange(role.code(), 0, Ordering::SeqCst, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_parse_and_conflicts_are_seen() {
        assert_eq!(Role::parse("server"), Role::Server);
        assert_eq!(Role::parse("off"), Role::Off);
        assert_eq!(Role::parse(""), Role::Client);
        assert_eq!(Role::parse("client,server"), Role::Client);
        assert_eq!(Role::parse("both"), Role::Client);
        for r in Role::CHOICES {
            assert_eq!(Role::parse(r.key()), r);
        }
    }

    #[test]
    fn one_role_at_a_time_both_ways() {
        let _g = serial();
        assert_eq!(active(), Role::Off);
        claim(Role::Client).unwrap();
        assert_eq!(claim(Role::Server), Err(Role::Client));
        assert!(claim(Role::Client).is_ok());
        release(Role::Server);
        assert_eq!(active(), Role::Client);
        release(Role::Client);
        claim(Role::Server).unwrap();
        assert_eq!(claim(Role::Client), Err(Role::Server));
        release(Role::Server);
        assert_eq!(claim(Role::Off), Err(Role::Off));
        assert_eq!(active(), Role::Off);
    }
}
