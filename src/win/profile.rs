pub struct ProfileInfo {
    pub user: String,
    pub path: String,
}

pub fn current() -> ProfileInfo {
    let name = std::env::var("USERNAME").unwrap_or_else(|_| "неизвестно".to_owned());
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let user = if domain.is_empty() || domain.eq_ignore_ascii_case(&name) {
        name
    } else {
        format!("{domain}\\{name}")
    };
    let path = std::env::var("USERPROFILE").unwrap_or_else(|_| "неизвестно".to_owned());
    ProfileInfo { user, path }
}
