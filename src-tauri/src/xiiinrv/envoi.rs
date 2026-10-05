//! Envoi périodique de la fiche au site de la guilde.
//!
//! Le module garde sa propre configuration, poussée depuis `lib.rs` au démarrage
//! et à chaque modification des réglages. Il ne connaît donc rien du reste de
//! l'application : s'il est retiré, rien d'autre ne bouge.
//!
//! Règles d'envoi :
//!   - jamais sans jeton, et jamais si le partage est décoché ;
//!   - au plus une fois toutes les quinze minutes ;
//!   - seulement si quelque chose a changé depuis le dernier envoi réussi.
//!
//! Le site limite de son côté à un envoi par minute et par jeton, et revérifie à
//! chaque fois que le compte est toujours membre de la guilde.

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{info, warn};

use super::collecte::{lire_etat, ouvrir_lecture, Etat};
use super::{CLE_PUBLIQUE, URL_PAR_DEFAUT};

/// Délai minimum entre deux envois. Une fiche de personnage ne change pas toutes
/// les minutes : quinze minutes suffisent largement et ménagent le serveur.
const DELAI_ENTRE_ENVOIS: Duration = Duration::from_secs(15 * 60);
/// Fréquence à laquelle on regarde s'il y a lieu d'envoyer.
const PERIODE_VERIFICATION: Duration = Duration::from_secs(60);

#[derive(Clone, Default)]
struct Config {
    jeton: Option<String>,
    url: Option<String>,
    actif: bool,
}

#[derive(Clone, Default)]
struct Suivi {
    dernier_envoi: Option<i64>,
    dernier_message: Option<String>,
    dernier_contenu: Option<String>,
}

static CONFIG: OnceLock<Mutex<Config>> = OnceLock::new();
static SUIVI: OnceLock<Mutex<Suivi>> = OnceLock::new();

fn config() -> &'static Mutex<Config> {
    CONFIG.get_or_init(|| Mutex::new(Config::default()))
}

fn suivi() -> &'static Mutex<Suivi> {
    SUIVI.get_or_init(|| Mutex::new(Suivi::default()))
}

fn maintenant() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Appelé au démarrage et dès qu'un réglage `xiiinrv_*` change.
pub fn configurer(jeton: Option<String>, url: Option<String>, actif: bool) {
    let jeton = jeton.map(|j| j.trim().to_string()).filter(|j| !j.is_empty());
    let mut c = config().lock();
    c.jeton = jeton;
    c.url = url.filter(|u| !u.trim().is_empty());
    c.actif = actif;
    // La lecture des paquets suit la case à cocher : c'est le choix explicite
    // du membre. Sans jeton, la fiche est lue et affichée dans le meter, mais
    // rien ne peut partir — l'envoi exige le jeton. Cela permet de vérifier que
    // la lecture fonctionne avant même d'avoir un jeton.
    ouvrir_lecture(c.actif);
}

/// Ce que l'interface affiche dans l'onglet « Guilde XIII NRV ».
#[derive(Serialize)]
pub struct EtatPartage {
    pub actif: bool,
    pub jeton_present: bool,
    pub personnage: Option<String>,
    /// Absent tant que la fiche `33 36` n'a pas été vue : l'interface s'en sert
    /// pour expliquer ce qui manque et comment l'obtenir.
    pub niveau: Option<u32>,
    pub pieces: usize,
    pub pets: usize,
    pub combat_power: Option<u32>,
    pub dernier_envoi: Option<i64>,
    pub dernier_message: Option<String>,
}

pub fn etat_partage() -> EtatPartage {
    let c = config().lock().clone();
    let s = suivi().lock().clone();
    let e = lire_etat();
    EtatPartage {
        actif: c.actif,
        jeton_present: c.jeton.is_some(),
        personnage: e.nom.clone(),
        niveau: e.niveau,
        pieces: e.equipement.len(),
        pets: e.pets.len(),
        combat_power: e.combat_power,
        dernier_envoi: s.dernier_envoi,
        dernier_message: s.dernier_message,
    }
}

/// Le contenu envoyé au site. Aucun nom d'objet : uniquement l'identifiant
/// officiel, l'emplacement et l'enchantement.
fn construire_contenu(jeton: &str, e: &Etat) -> Option<String> {
    let nom = e.nom.clone()?;
    let personnage = serde_json::json!({
        "nom": nom,
        "serveur": e.serveur,
        "niveau": e.niveau,
        "itemLevel": e.item_level,
        "combatPower": e.combat_power,
        "pv": e.pv,
        "pm": e.pm,
    });
    serde_json::to_string(&serde_json::json!({
        "token": jeton,
        "personnage": personnage,
        "equipement": e.equipement,
        "pets": e.pets,
    }))
    .ok()
}

/// Empreinte du contenu utile, sans le jeton : sert à savoir si quelque chose a
/// changé depuis le dernier envoi réussi.
fn signature(e: &Etat) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        e.nom, e.niveau, e.item_level, e.combat_power, e.pv, e.equipement, e.pets
    )
}

/// Envoi immédiat, déclenché par le bouton « Envoyer maintenant ».
pub async fn envoyer_maintenant() -> Result<String, String> {
    let c = config().lock().clone();
    let jeton = c.jeton.ok_or_else(|| "Aucun jeton renseigné.".to_string())?;
    let etat = lire_etat();
    if !etat.envoyable() {
        return Err(
            "Rien à envoyer pour l'instant : entrez en jeu avec votre personnage, le meter lira sa fiche."
                .to_string(),
        );
    }
    envoyer(&jeton, c.url.as_deref().unwrap_or(URL_PAR_DEFAUT), &etat).await
}

async fn envoyer(jeton: &str, url: &str, etat: &Etat) -> Result<String, String> {
    let contenu = construire_contenu(jeton, etat).ok_or_else(|| "Fiche incomplète.".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    // La bibliothèque HTTP est intégrée sans l'option `json` : on sérialise
    // nous-mêmes et on pose l'en-tête à la main.
    let reponse = client
        .post(url)
        .header("Content-Type", "application/json")
        // Supabase peut exiger un jeton d'accès sur ses fonctions Edge. On
        // présente la clé publique du projet, celle que le site sert déjà à
        // tout le monde : l'envoi passe quel que soit le réglage, et la vraie
        // authentification reste le jeton personnel, vérifié en base.
        .header("apikey", CLE_PUBLIQUE)
        .header("Authorization", format!("Bearer {}", CLE_PUBLIQUE))
        .body(contenu)
        .send()
        .await
        .map_err(|e| format!("Envoi impossible : {}", e))?;

    let statut = reponse.status();
    let corps = reponse.text().await.unwrap_or_default();

    let mut s = suivi().lock();
    if statut.is_success() {
        s.dernier_envoi = Some(maintenant());
        s.dernier_contenu = Some(signature(etat));
        s.dernier_message = Some("Fiche envoyée".to_string());
        info!("XIII NRV : fiche envoyée ({} octets de réponse)", corps.len());
        Ok("Fiche envoyée".to_string())
    } else {
        // Le site renvoie un message clair : jeton inconnu ou révoqué, compte
        // qui n'est plus membre, ou trop d'envois.
        let message = extraire_message(&corps).unwrap_or_else(|| format!("Erreur {}", statut));
        s.dernier_message = Some(message.clone());
        warn!("XIII NRV : envoi refusé ({}) — {}", statut, message);
        Err(message)
    }
}

fn extraire_message(corps: &str) -> Option<String> {
    let valeur: serde_json::Value = serde_json::from_str(corps).ok()?;
    valeur
        .get("error")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Boucle de fond. Elle ne fait rien tant qu'il n'y a ni jeton ni partage coché.
/// Démarre le partage, **après** avoir appliqué les réglages enregistrés.
///
/// Les réglages sont exigés en paramètre, et ce n'est pas un détail de style :
/// avant le 05/10/2026, `configurer()` n'était appelé que lorsqu'on touchait un
/// réglage dans l'interface. Au lancement, personne ne relisait `settings.json`,
/// le partage restait inactif en mémoire et la lecture des paquets était fermée
/// — alors que la case de l'interface s'affichait cochée, puisqu'elle lit le
/// fichier. Plus rien n'était lu tant que le membre n'allait pas décocher puis
/// recocher sa case, ce qui ressemblait à des pertes de reconnaissance
/// aléatoires.
///
/// En les prenant en paramètre, la signature rend cet oubli impossible : on ne
/// peut plus démarrer sans dire dans quel état.
pub fn demarrer(jeton: Option<String>, url: Option<String>, actif: bool) {
    configurer(jeton, url, actif);
    demarrer_la_boucle();
}

fn demarrer_la_boucle() {
    // Même mécanisme que le reste d'A2Tools : c'est la boucle asynchrone de
    // Tauri qui héberge la tâche, pas un runtime que nous créerions nous-mêmes.
    tauri::async_runtime::spawn(async move {
        let mut horloge = tokio::time::interval(PERIODE_VERIFICATION);
        loop {
            horloge.tick().await;

            let c = config().lock().clone();
            let (Some(jeton), true) = (c.jeton.clone(), c.actif) else {
                continue;
            };

            // Ajout XIII NRV : avant de regarder s'il y a lieu d'envoyer, on
            // reprend l'Item Level et le Combat Power dans la composition du
            // groupe. L'inventaire et le défilement du Combat Power n'arrivent
            // qu'à l'entrée en jeu ; sans cela, une fiche reste celle du moment
            // où le membre est entré, et un changement d'équipement en cours de
            // soirée ne se voit jamais.
            super::collecte::rafraichir_depuis_le_groupe();

            let etat = lire_etat();
            if !etat.pret() {
                continue;
            }

            {
                let s = suivi().lock();
                if let Some(dernier) = s.dernier_envoi {
                    if maintenant() - dernier < DELAI_ENTRE_ENVOIS.as_secs() as i64 {
                        continue;
                    }
                }
                if s.dernier_contenu.as_deref() == Some(signature(&etat).as_str()) {
                    continue; // rien n'a changé
                }
            }

            let _ = envoyer(&jeton, c.url.as_deref().unwrap_or(URL_PAR_DEFAUT), &etat).await;
        }
    });
}
