use crate::opts;
use crate::url::ParsedUrl;

static REDIRECTED_DOMAINS: &[&str] = &[
    "ol.epicgames.com",
    "ol.epicgames.net",
    "on.epicgames.com",
    "game-social.epicgames.com",
    "ak.epicgames.com",
    "epicgames.dev",
    "superawesome.com",
];

static REDIRECTED_PATHS: &[&str] = &[
    "/fortnite/api/v2/versioncheck/",
    "/fortnite/api/game/v2/profile/",
    "/content/api/pages/",
    "/affiliate/api/public/affiliates/slug",
    "/socialban/api/public/v1",
    "/fortnite/api/cloudstorage/system",
];

static REDIRECTED_PATHS_DEV: &[&str] = &[
    "/fortnite/api/game/v2/profile/",
    "/affiliate/api/public/affiliates/slug",
    "/content/api/pages/",
];

pub fn should_redirect(url: &ParsedUrl) -> bool {
    match opts::URL_SET {
        opts::UrlSet::Default => {
            for &domain in REDIRECTED_DOMAINS {
                if domain_ends_with(&url.domain, domain) {
                    return true;
                }
            }
            false
        }
        opts::UrlSet::Hybrid => {
            for &path in REDIRECTED_PATHS {
                if path_starts_with(&url.path, path) {
                    return true;
                }
            }
            false
        }
        opts::UrlSet::Dev => {
            for &path in REDIRECTED_PATHS_DEV {
                if path_starts_with(&url.path, path) {
                    return true;
                }
            }
            false
        }
        opts::UrlSet::All => true,
    }
}

fn domain_ends_with(domain: &crate::unreal::FString, suffix: &str) -> bool {
    if domain.string.is_null() || domain.length <= 1 {
        return false;
    }

    let domain_len = (domain.length - 1) as usize;
    let suffix_bytes = suffix.as_bytes();

    if domain_len < suffix_bytes.len() {
        return false;
    }

    unsafe {
        let offset = domain_len - suffix_bytes.len();
        for (i, &b) in suffix_bytes.iter().enumerate() {
            if *domain.string.add(offset + i) != b as u16 {
                return false;
            }
        }
    }
    true
}

fn path_starts_with(path: &crate::unreal::FString, prefix: &str) -> bool {
    if path.string.is_null() || path.length <= 1 {
        return false;
    }

    let path_len = (path.length - 1) as usize;
    let prefix_bytes = prefix.as_bytes();

    if path_len < prefix_bytes.len() {
        return false;
    }

    unsafe {
        for (i, &b) in prefix_bytes.iter().enumerate() {
            if *path.string.add(i) != b as u16 {
                return false;
            }
        }
    }
    true
}
