//! Téléchargement des fonds de carte et des données du jeu depuis le site.
//!
//! Contrat : `1_aion/contrats/meter-site.md`, « Canal site → meter », version 2.
//!
//! Le dépôt du meter est public : les fonds de carte n'y figurent pas. Le site les
//! sert par une fonction qui vérifie le jeton, et répond par un manifeste dont
//! chaque fichier porte une URL signée valable quinze minutes.
//!
//! Tout ce qui vient du réseau est traité comme une donnée à contrôler :
//!   - un chemin du manifeste ne sort jamais du dossier de cache (pas de `..`,
//!     pas de chemin absolu, pas de lecteur Windows) ;
//!   - une URL n'est suivie que si elle est en `https` ;
//!   - un fichier n'est gardé que si son empreinte SHA-256 est celle annoncée ;
//!   - l'écriture passe par un fichier temporaire, puis un renommage.
//!
//! Un fichier refusé n'arrête pas les autres, et rien ici ne bloque le meter :
//! sans réseau, sans jeton ou sur erreur, le cache existant reste en place.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

use super::{CLE_PUBLIQUE, URL_PAR_DEFAUT};

/// Adresse de la fonction, déduite de celle de la réception : même projet.
pub fn url_depuis(url_reception: &str) -> String {
    url_reception.replace("/meter-push", "/game-assets")
}

pub fn url_par_defaut() -> String {
    url_depuis(URL_PAR_DEFAUT)
}

/// Fréquence à laquelle on regarde s'il y a lieu d'appeler. L'appel lui-même
/// reste limité à un par jour par `synchroniser`.
const PERIODE_VERIFICATION: Duration = Duration::from_secs(10 * 60);

/// Boucle de fond. Elle ne fait rien sans jeton ni partage coché, et une erreur
/// ne touche à rien d'autre : le meter garde son cache et continue.
pub fn demarrer(dossier: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let mut horloge = tokio::time::interval(PERIODE_VERIFICATION);
        loop {
            horloge.tick().await;
            let Some((jeton, url)) = super::envoi::reglages_actifs() else {
                continue;
            };
            match synchroniser(&jeton, &url_depuis(&url), "carte", &dossier).await {
                Ok(_) => {}
                Err(e) => warn!("XIII NRV : {}", e),
            }
        }
    });
}

/// Le contrat limite à un appel par heure et par jeton ; le meter n'en fait qu'un
/// par jour.
const DELAI_ENTRE_APPELS: i64 = 24 * 3600;
/// Le manifeste fait 256 Ko au plus (contrat).
const TAILLE_MAX_MANIFESTE: usize = 256 * 1024;
/// Plafond d'un fichier : le plus gros fond annoncé tourne autour de 1,2 Mo.
const TAILLE_MAX_FICHIER: u64 = 32 * 1024 * 1024;

const FICHIER_VERSION: &str = "version.txt";
const FICHIER_APPEL: &str = "dernier_appel.txt";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Fichier {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifeste {
    pub version: String,
    #[serde(default)]
    pub unchanged: bool,
    #[serde(default)]
    pub files: Vec<Fichier>,
}

/// Ce qui s'est passé, pour le journal.
#[derive(Debug, Clone, PartialEq)]
pub enum Bilan {
    /// Trop tôt depuis le dernier appel : rien n'a été demandé.
    Differe,
    /// Le serveur a répondu `unchanged: true`.
    DejaAJour(String),
    /// Manifeste traité : fichiers écrits, déjà présents, refusés.
    Synchronise { version: String, ecrits: usize, presents: usize, refuses: usize },
}

/// Joint un chemin du manifeste au dossier de cache, ou refuse.
///
/// Seuls des composants simples sont acceptés : un `..`, un chemin absolu, un
/// antislash ou un lecteur (`C:`) feraient écrire hors du dossier.
pub fn chemin_sur(dossier: &Path, relatif: &str) -> Option<PathBuf> {
    if relatif.is_empty() || relatif.len() > 200 || relatif.contains('\\') || relatif.contains(':') {
        return None;
    }
    let mut sortie = dossier.to_path_buf();
    for composant in Path::new(relatif).components() {
        match composant {
            Component::Normal(c) => {
                let c = c.to_str()?;
                if c.is_empty() || c.chars().any(|ch| ch.is_control()) {
                    return None;
                }
                sortie.push(c);
            }
            _ => return None,
        }
    }
    if sortie == dossier {
        return None;
    }
    Some(sortie)
}

fn url_acceptable(url: &str) -> bool {
    url.starts_with("https://") && url.len() <= 2000 && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn empreinte(octets: &[u8]) -> String {
    Sha256::digest(octets).iter().map(|b| format!("{b:02x}")).collect()
}

fn empreinte_valide(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Le fichier local est-il déjà celui qu'annonce le manifeste ?
fn deja_present(dossier: &Path, f: &Fichier) -> bool {
    let Some(chemin) = chemin_sur(dossier, &f.path) else {
        return false;
    };
    match std::fs::read(chemin) {
        Ok(octets) => empreinte(&octets).eq_ignore_ascii_case(&f.sha256),
        Err(_) => false,
    }
}

/// Faut-il appeler le site maintenant ? `dernier` : horodatage du dernier appel.
pub fn doit_appeler(dernier: Option<i64>, maintenant: i64) -> bool {
    match dernier {
        Some(d) => maintenant - d >= DELAI_ENTRE_APPELS || maintenant < d,
        None => true,
    }
}

fn lire_nombre(chemin: &Path) -> Option<i64> {
    std::fs::read_to_string(chemin).ok()?.trim().parse().ok()
}

fn lire_texte(chemin: &Path) -> Option<String> {
    let t = std::fs::read_to_string(chemin).ok()?;
    let t = t.trim();
    (!t.is_empty() && t.len() <= 64).then(|| t.to_string())
}

/// Écrit `octets` si, et seulement si, leur empreinte est celle annoncée.
fn ecrire_verifie(dossier: &Path, f: &Fichier, octets: &[u8]) -> Result<(), String> {
    if octets.len() as u64 != f.bytes {
        return Err(format!("{} : taille reçue {} au lieu de {}", f.path, octets.len(), f.bytes));
    }
    if !empreinte(octets).eq_ignore_ascii_case(&f.sha256) {
        return Err(format!("{} : empreinte différente de celle annoncée", f.path));
    }
    let chemin = chemin_sur(dossier, &f.path).ok_or_else(|| format!("{} : chemin refusé", f.path))?;
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporaire = chemin.with_extension("part");
    std::fs::write(&temporaire, octets).map_err(|e| e.to_string())?;
    std::fs::rename(&temporaire, &chemin).map_err(|e| {
        let _ = std::fs::remove_file(&temporaire);
        e.to_string()
    })
}

/// Traite un manifeste : télécharge ce qui manque ou diffère, avec `lire` pour
/// aller chercher une URL (injectable, pour les tests).
async fn appliquer<F, Fut>(dossier: &Path, m: &Manifeste, lire: F) -> Bilan
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    if m.unchanged {
        return Bilan::DejaAJour(m.version.clone());
    }
    let (mut ecrits, mut presents, mut refuses) = (0, 0, 0);
    for f in &m.files {
        if chemin_sur(dossier, &f.path).is_none()
            || !empreinte_valide(&f.sha256)
            || f.bytes > TAILLE_MAX_FICHIER
            || !url_acceptable(&f.url)
        {
            warn!("XIII NRV : fichier du manifeste écarté ({})", f.path.chars().take(80).collect::<String>());
            refuses += 1;
            continue;
        }
        if deja_present(dossier, f) {
            presents += 1;
            continue;
        }
        let resultat = match lire(f.url.clone()).await {
            Ok(octets) => ecrire_verifie(dossier, f, &octets),
            Err(e) => Err(format!("{} : {}", f.path, e)),
        };
        match resultat {
            Ok(()) => ecrits += 1,
            Err(e) => {
                warn!("XIII NRV : {}", e);
                refuses += 1;
            }
        }
    }
    // La version n'est retenue que si tout est en place : sinon le prochain cycle
    // redemande le manifeste complet (les fichiers déjà bons seront sautés).
    if refuses == 0 && !m.version.is_empty() && m.version.len() <= 64 {
        let _ = std::fs::write(dossier.join(FICHIER_VERSION), &m.version);
    }
    Bilan::Synchronise { version: m.version.clone(), ecrits, presents, refuses }
}

/// Un appel complet : respecte le délai d'un jour, interroge le site, télécharge.
///
/// `set` vaut `carte`, `donnees` ou `tout` (voir le contrat). Les erreurs
/// remontent en texte pour le journal ; l'appelant ne doit jamais s'en servir
/// pour bloquer autre chose.
pub async fn synchroniser(jeton: &str, url: &str, set: &str, dossier: &Path) -> Result<Bilan, String> {
    let maintenant = chrono::Utc::now().timestamp();
    if !doit_appeler(lire_nombre(&dossier.join(FICHIER_APPEL)), maintenant) {
        return Ok(Bilan::Differe);
    }
    std::fs::create_dir_all(dossier).map_err(|e| e.to_string())?;
    // Posé avant l'appel : une réponse 429 ou une panne ne doit pas provoquer
    // une rafale de nouvelles tentatives.
    let _ = std::fs::write(dossier.join(FICHIER_APPEL), maintenant.to_string());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let mut corps = serde_json::json!({ "token": jeton, "set": set });
    if let Some(v) = lire_texte(&dossier.join(FICHIER_VERSION)) {
        corps["knownVersion"] = serde_json::Value::String(v);
    }
    let reponse = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("apikey", CLE_PUBLIQUE)
        .header("Authorization", format!("Bearer {}", CLE_PUBLIQUE))
        .body(corps.to_string())
        .send()
        .await
        .map_err(|e| {
            // Rien n'a atteint le serveur : pas de raison d'attendre un jour.
            let _ = std::fs::remove_file(dossier.join(FICHIER_APPEL));
            format!("Fonds de carte : appel impossible : {}", e)
        })?;

    let statut = reponse.status();
    let texte = reponse.text().await.unwrap_or_default();
    if !statut.is_success() {
        let message = serde_json::from_str::<serde_json::Value>(&texte)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_else(|| "sans détail".to_string());
        return Err(format!("Fonds de carte : refus {} ({})", statut.as_u16(), message));
    }
    if texte.len() > TAILLE_MAX_MANIFESTE {
        return Err("Fonds de carte : manifeste trop gros, ignoré".to_string());
    }
    let manifeste: Manifeste =
        serde_json::from_str(&texte).map_err(|e| format!("Fonds de carte : manifeste illisible : {}", e))?;

    let bilan = appliquer(dossier, &manifeste, |url| {
        let client = client.clone();
        async move {
            let r = client.get(&url).send().await.map_err(|e| e.to_string())?;
            if !r.status().is_success() {
                return Err(format!("téléchargement refusé ({})", r.status().as_u16()));
            }
            r.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
        }
    })
    .await;
    info!("XIII NRV : fonds de carte — {:?}", bilan);
    Ok(bilan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xiiinrv-fonds-test-{nom}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fichier(path: &str, contenu: &[u8]) -> Fichier {
        Fichier {
            path: path.to_string(),
            sha256: empreinte(contenu),
            bytes: contenu.len() as u64,
            url: format!("https://exemple.invalide/{path}"),
        }
    }

    #[test]
    fn un_chemin_du_manifeste_ne_sort_jamais_du_dossier() {
        let d = Path::new("cache");
        assert_eq!(
            chemin_sur(d, "carte/fonds/World_D_A.webp"),
            Some(d.join("carte").join("fonds").join("World_D_A.webp"))
        );
        for mauvais in [
            "../secret", "carte/../../secret", "/etc/passwd", "C:/Windows/x", "C:\\x",
            "carte\\..\\x", "", ".", "carte/./../..", "a\u{0}b",
        ] {
            assert_eq!(chemin_sur(d, mauvais), None, "chemin accepté : {mauvais:?}");
        }
    }

    #[test]
    fn l_appel_est_differe_dans_les_vingt_quatre_heures() {
        assert!(doit_appeler(None, 1_000));
        assert!(!doit_appeler(Some(1_000), 1_000 + 3_600));
        assert!(doit_appeler(Some(1_000), 1_000 + 24 * 3_600));
        // Horloge revenue en arrière : on ne reste pas bloqué.
        assert!(doit_appeler(Some(5_000), 1_000));
    }

    #[tokio::test]
    async fn le_bon_fichier_est_ecrit_le_mauvais_est_refuse_et_la_version_attend() {
        let d = dossier("empreinte");
        let bon = fichier("carte/carte.json", b"{\"ok\":true}");
        let mut mauvais = fichier("carte/fonds/A.webp", b"contenu attendu");
        mauvais.sha256 = empreinte(b"autre chose");
        let m = Manifeste {
            version: "113-2026-10-01".into(),
            unchanged: false,
            files: vec![bon.clone(), mauvais],
        };
        let bilan = appliquer(&d, &m, |url| async move {
            Ok(if url.ends_with("carte.json") { b"{\"ok\":true}".to_vec() } else { b"contenu attendu".to_vec() })
        })
        .await;
        assert_eq!(bilan, Bilan::Synchronise { version: m.version.clone(), ecrits: 1, presents: 0, refuses: 1 });
        assert!(d.join("carte/carte.json").exists());
        assert!(!d.join("carte/fonds/A.webp").exists(), "un fichier à l'empreinte fausse ne doit pas être gardé");
        assert!(!d.join(FICHIER_VERSION).exists(), "la version ne doit pas être retenue avec un refus");
    }

    #[tokio::test]
    async fn un_fichier_deja_bon_n_est_pas_retelecharge_et_la_version_est_retenue() {
        let d = dossier("cache");
        let f = fichier("envobjs.json", b"[1,2,3]");
        std::fs::write(d.join("envobjs.json"), b"[1,2,3]").unwrap();
        let m = Manifeste { version: "113-2026-10-01".into(), unchanged: false, files: vec![f] };
        let bilan = appliquer(&d, &m, |_| async { Err("ne doit pas être appelé".to_string()) }).await;
        assert_eq!(bilan, Bilan::Synchronise { version: m.version.clone(), ecrits: 0, presents: 1, refuses: 0 });
        assert_eq!(lire_texte(&d.join(FICHIER_VERSION)).as_deref(), Some("113-2026-10-01"));
    }

    #[tokio::test]
    async fn unchanged_ne_telecharge_rien() {
        let d = dossier("unchanged");
        let m: Manifeste =
            serde_json::from_str(r#"{"version":"113-2026-10-01","unchanged":true,"files":[]}"#).unwrap();
        let bilan = appliquer(&d, &m, |_| async { Err("ne doit pas être appelé".to_string()) }).await;
        assert_eq!(bilan, Bilan::DejaAJour("113-2026-10-01".into()));
    }

    #[tokio::test]
    async fn un_manifeste_piege_est_ecarte_sans_rien_ecrire_dehors() {
        let d = dossier("piege");
        let mut evil = fichier("../evasion.txt", b"x");
        evil.url = "https://exemple.invalide/x".into();
        let mut http = fichier("carte/a.json", b"x");
        http.url = "http://exemple.invalide/a".into();
        let m = Manifeste { version: "v".into(), unchanged: false, files: vec![evil, http] };
        let bilan = appliquer(&d, &m, |_| async { Ok(b"x".to_vec()) }).await;
        assert_eq!(bilan, Bilan::Synchronise { version: "v".into(), ecrits: 0, presents: 0, refuses: 2 });
        assert!(!d.parent().unwrap().join("evasion.txt").exists());
    }

    /// Constat réel contre le site (contrat v2 : 429 si deux appels dans l'heure,
    /// `unchanged: true` si la version connue est la courante). Ignoré par défaut :
    /// il exige un vrai jeton, passé par XIIINRV_JETON_ESSAI, jamais écrit dans un
    /// fichier. Le dossier de cache est **conservé** entre deux lancements, pour
    /// que le second lancement, plus d'une heure après le premier appel réussi,
    /// présente sa `knownVersion` et reçoive `unchanged`.
    ///   cargo test --lib constat_reel -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn constat_reel_429_puis_unchanged() {
        let jeton = std::env::var("XIIINRV_JETON_ESSAI").expect("XIIINRV_JETON_ESSAI non défini");
        let d = std::env::temp_dir().join("xiiinrv-fonds-reel");
        // Le délai d'un jour du meter est levé : on veut la réponse du serveur.
        let _ = std::fs::remove_file(d.join(FICHIER_APPEL));
        let version_avant = lire_texte(&d.join(FICHIER_VERSION));
        println!("version en cache avant l'appel : {version_avant:?}");

        let r = synchroniser(&jeton, &url_par_defaut(), "carte", &d).await;
        println!("appel : {r:?}");
        match r {
            Err(e) => assert!(e.contains("429"), "refus inattendu : {e}"),
            Ok(Bilan::Synchronise { refuses, .. }) => assert_eq!(refuses, 0),
            Ok(Bilan::DejaAJour(_)) => assert!(version_avant.is_some()),
            Ok(Bilan::Differe) => panic!("le délai aurait dû être levé"),
        }
    }
}
