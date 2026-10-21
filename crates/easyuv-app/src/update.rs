//! Mises à jour automatiques depuis les releases GitHub.
//!
//! Chaque release publie `EasyUV-windows-x64.exe` et `EasyUV-windows-x64.exe.sha256`.
//! L'application compare la version de la dernière release à la sienne, télécharge le nouvel
//! exécutable, vérifie son empreinte SHA-256 puis se remplace elle-même et redémarre.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const REPO: &str = "lburdier/EasyUV";
pub const EXE_ASSET: &str = "EasyUV-windows-x64.exe";

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<GhAsset>,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    exe_url: String,
    sha_url: Option<String>,
}

/// Version courante ; `EASYUV_FAKE_VERSION` permet de tester la mise à jour.
pub fn current_version() -> String {
    std::env::var("EASYUV_FAKE_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string())
}

pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>());
    Some((it.next()?.ok()?, it.next().unwrap_or(Ok(0)).ok()?, it.next().unwrap_or(Ok(0)).ok()?))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into()
}

/// Dernière release publiée plus récente que la version courante, s'il y en a une.
pub fn check() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = agent()
        .get(&url)
        .header("User-Agent", concat!("EasyUV/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("vérification des mises à jour : {e}"))?;
    let rel: GhRelease = resp.body_mut().read_json().map_err(|e| format!("réponse GitHub illisible : {e}"))?;
    if rel.draft || rel.prerelease || !is_newer(&rel.tag_name, &current_version()) {
        return Ok(None);
    }
    let Some(exe) = rel.assets.iter().find(|a| a.name == EXE_ASSET) else {
        return Ok(None);
    };
    let sha = rel.assets.iter().find(|a| a.name == format!("{EXE_ASSET}.sha256"));
    Ok(Some(Release {
        version: rel.tag_name.trim_start_matches('v').to_string(),
        notes: rel.body.unwrap_or_default(),
        page: rel.html_url,
        exe_url: exe.browser_download_url.clone(),
        sha_url: sha.map(|a| a.browser_download_url.clone()),
    }))
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut resp = agent()
        .get(url)
        .header("User-Agent", concat!("EasyUV/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("téléchargement : {e}"))?;
    resp.body_mut().with_config().limit(512 * 1024 * 1024).read_to_vec().map_err(|e| format!("téléchargement : {e}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Télécharge, vérifie et installe la release à la place de l'exécutable courant.
/// Retourne le chemin de l'exécutable (à relancer).
pub fn install(rel: &Release) -> Result<PathBuf, String> {
    let data = download(&rel.exe_url)?;
    if data.len() < 1024 || &data[0..2] != b"MZ" {
        return Err("le fichier téléchargé n'est pas un exécutable Windows".into());
    }
    let sha_url = rel.sha_url.as_ref().ok_or("empreinte SHA-256 absente de la release : mise à jour refusée")?;
    let expected = String::from_utf8_lossy(&download(sha_url)?).split_whitespace().next().unwrap_or("").to_lowercase();
    let actual = hex(&Sha256::digest(&data));
    if expected != actual {
        return Err(format!("empreinte invalide (attendue {expected}, obtenue {actual}) : mise à jour refusée"));
    }
    let tmp = std::env::temp_dir().join(format!("EasyUV-{}.exe", rel.version));
    std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
    self_replace::self_replace(&tmp).map_err(|e| format!("remplacement de l'exécutable : {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    std::env::current_exe().map_err(|e| e.to_string())
}

/// Relance l'application (après installation).
pub fn restart(exe: &PathBuf) -> ! {
    let _ = std::process::Command::new(exe).spawn();
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0-beta", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
    }
}
