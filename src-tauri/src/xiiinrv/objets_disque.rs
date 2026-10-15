//! Sauvegarde locale des objets vus (`objets_vus.json` dans le dossier de données).
//!
//! Les positions restent sur la machine du membre : ce module ne touche ni au
//! réseau ni au partage. Le décodage et l'état sont dans
//! `capture::objets_vus` ; ici, seulement le fichier.
//!
//! Écriture atomique (fichier temporaire puis renommage) et lecture tolérante :
//! un fichier absent, vide ou abîmé donne simplement une carte vide, sans
//! jamais empêcher le meter de démarrer.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

use crate::capture::objets_vus::{self, ObjetVu};

/// Fréquence à laquelle on regarde s'il y a quelque chose à écrire.
const PERIODE: Duration = Duration::from_secs(15);

#[derive(Serialize, Deserialize, Default)]
struct Fichier {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    objets: Vec<ObjetVu>,
}

/// Lit le fichier ; toute erreur donne une liste vide.
pub fn lire(chemin: &Path) -> Vec<ObjetVu> {
    match std::fs::read_to_string(chemin) {
        Ok(texte) => match serde_json::from_str::<Fichier>(&texte) {
            Ok(f) => f.objets,
            Err(e) => {
                // On garde une copie du fichier abîmé : la sauvegarde suivante
                // écrirait sinon par-dessus tout l'historique.
                let copie = chemin.with_extension("abime");
                let _ = std::fs::copy(chemin, &copie);
                warn!("XIII NRV : objets vus illisibles, copie gardée dans {} ({})", copie.display(), e);
                Vec::new()
            }
        },
        Err(_) => Vec::new(),
    }
}

/// Écrit le fichier de façon atomique.
pub fn ecrire(chemin: &Path, objets: &[ObjetVu]) -> Result<(), String> {
    let contenu = serde_json::to_string(&Fichier { version: 1, objets: objets.to_vec() }).map_err(|e| e.to_string())?;
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporaire = chemin.with_extension("tmp");
    std::fs::write(&temporaire, contenu).map_err(|e| e.to_string())?;
    std::fs::rename(&temporaire, chemin).map_err(|e| {
        let _ = std::fs::remove_file(&temporaire);
        e.to_string()
    })
}

/// Recharge le fichier puis sauvegarde en fond dès que l'état conservé change.
pub fn demarrer(chemin: PathBuf) {
    let deja = lire(&chemin);
    info!("XIII NRV : {} objet(s) vu(s) rechargé(s) depuis {}", deja.len(), chemin.display());
    objets_vus::charger(deja);
    tauri::async_runtime::spawn(async move {
        let mut horloge = tokio::time::interval(PERIODE);
        loop {
            horloge.tick().await;
            if objets_vus::a_sauvegarder() {
                if let Err(e) = ecrire(&chemin, &objets_vus::a_conserver()) {
                    warn!("XIII NRV : objets vus non enregistrés : {}", e);
                    // Réessayé au prochain passage.
                    objets_vus::marquer_a_sauvegarder();
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::objets_vus::Genre;

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xiiinrv-objets-test-{nom}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn objet(modele: u32) -> ObjetVu {
        ObjetVu { genre: Genre::Decor, modele, x: 110192.0, y: 76199.0, z: 1286.0, vu_a: 42, fois_vu: 3 }
    }

    #[test]
    fn ce_qui_est_ecrit_est_relu_a_l_identique_sans_fichier_temporaire() {
        let d = dossier("aller-retour");
        let f = d.join("objets_vus.json");
        ecrire(&f, &[objet(1_300_908), objet(7)]).unwrap();
        assert_eq!(lire(&f), vec![objet(1_300_908), objet(7)]);
        assert!(!d.join("objets_vus.tmp").exists());
    }

    #[test]
    fn un_fichier_absent_ou_abime_donne_une_liste_vide() {
        let d = dossier("abime");
        assert!(lire(&d.join("absent.json")).is_empty());
        std::fs::write(d.join("casse.json"), "{ pas du json").unwrap();
        assert!(lire(&d.join("casse.json")).is_empty());
        // Le fichier abîmé est gardé à côté, pas écrasé en silence.
        assert!(d.join("casse.abime").exists());
        std::fs::write(d.join("vide.json"), "").unwrap();
        assert!(lire(&d.join("vide.json")).is_empty());
    }

    #[test]
    fn un_fichier_d_une_version_future_avec_des_champs_en_plus_reste_lisible() {
        let d = dossier("futur");
        let f = d.join("objets_vus.json");
        std::fs::write(
            &f,
            r#"{"version":9,"nouveau":true,"objets":[{"genre":"decor","modele":5,"x":1.0,"y":2.0,"z":3.0,"vu_a":1,"fois_vu":1,"champ":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(lire(&f).len(), 1);
    }
}
