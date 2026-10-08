//! Dire au membre qu'une version plus récente existe. Rien de plus.
//!
//! Le vérificateur d'A2Tools a été retiré de ce fork parce qu'il **téléchargeait
//! et exécutait** : il prenait une adresse de MSI sur un serveur tiers, le
//! téléchargeait et lançait `msiexec /passive`, sans vérifier ni signature ni
//! empreinte. Qui contrôlait ce serveur installait ce qu'il voulait sur le
//! poste de chaque membre.
//!
//! Celui-ci ne fait qu'une chose : lire un petit fichier JSON et comparer deux
//! numéros. **Il ne télécharge rien, il n'exécute rien, il n'écrit rien.** Au
//! pire, le site renvoie n'importe quoi et le meter l'ignore.
//!
//! Le fichier attendu, à poser sur le site de la guilde à côté du MSI :
//!
//! ```json
//! { "version": "2.0.75", "page": "https://…/meter" }
//! ```
//!
//! `page` est facultatif : c'est l'adresse que le bandeau proposera d'ouvrir.
//! Elle est ouverte **dans le navigateur du membre**, jamais dans le meter, et
//! seulement s'il clique.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Clé de réglage : l'adresse du fichier de version. Vide, rien n'est vérifié.
pub const CLE_URL_VERSION: &str = "xiiinrv_url_version";

/// Ce que le site annonce.
#[derive(Debug, Clone, Deserialize)]
struct Annonce {
    version: String,
    #[serde(default)]
    page: Option<String>,
}

/// Ce que le meter en dit à la page.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Verdict {
    /// La version installée.
    pub actuelle: String,
    /// Celle que le site annonce, si elle est lisible.
    pub annoncee: Option<String>,
    /// Faut-il montrer le bandeau ?
    pub a_jour: bool,
    /// L'adresse à proposer, si le site en donne une et qu'elle est sûre.
    pub page: Option<String>,
}

/// Découpe `2.0.75` en `[2, 0, 75]`. Rend `None` sur tout le reste.
///
/// Volontairement strict : trois nombres, rien d'autre. Un numéro fantaisiste
/// venu du réseau ne doit pas pouvoir se faire passer pour une version.
fn decouper(v: &str) -> Option<[u32; 3]> {
    let mut sortie = [0u32; 3];
    let mut morceaux = v.trim().split('.');
    for case in sortie.iter_mut() {
        let m = morceaux.next()?;
        if m.is_empty() || m.len() > 6 || !m.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *case = m.parse().ok()?;
    }
    if morceaux.next().is_some() {
        return None;
    }
    Some(sortie)
}

/// `annoncee` est-elle postérieure à `actuelle` ?
pub fn est_plus_recente(annoncee: &str, actuelle: &str) -> bool {
    match (decouper(annoncee), decouper(actuelle)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Une adresse qu'on accepte de proposer au membre.
///
/// Seulement `https`, et pas d'identifiants dans l'adresse. Le site est censé
/// être le nôtre ; ce contrôle existe pour le jour où il ne l'est plus.
fn page_acceptable(page: &str) -> Option<String> {
    let page = page.trim();
    if !page.starts_with("https://") || page.len() > 500 || page.contains('@') {
        return None;
    }
    if page.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some(page.to_string())
}

/// Lit l'annonce du site et rend le verdict.
///
/// Toute erreur — adresse vide, site injoignable, JSON illisible, numéro
/// fantaisiste — donne « à jour ». Le silence vaut mieux qu'un faux bandeau.
pub async fn verifier(url: Option<String>, actuelle: &str) -> Verdict {
    let muet = Verdict {
        actuelle: actuelle.to_string(),
        annoncee: None,
        a_jour: true,
        page: None,
    };
    let Some(url) = url.filter(|u| u.trim().starts_with("https://")) else {
        return muet;
    };

    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    else {
        return muet;
    };
    let Ok(reponse) = client.get(url.trim()).send().await else {
        return muet;
    };
    if !reponse.status().is_success() {
        return muet;
    }
    // Un plafond : le fichier attendu fait quelques dizaines d'octets, et rien
    // n'oblige à avaler ce qu'un serveur décide d'envoyer.
    let Ok(corps) = reponse.text().await else {
        return muet;
    };
    if corps.len() > 4096 {
        return muet;
    }
    let Ok(annonce) = serde_json::from_str::<Annonce>(&corps) else {
        return muet;
    };
    let Some(_) = decouper(&annonce.version) else {
        return muet;
    };

    let a_jour = !est_plus_recente(&annonce.version, actuelle);
    Verdict {
        actuelle: actuelle.to_string(),
        annoncee: Some(annonce.version),
        a_jour,
        page: annonce.page.as_deref().and_then(page_acceptable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La comparaison des numéros, y compris là où elle doit refuser.
    #[test]
    fn une_version_plus_recente_est_reconnue_et_le_reste_est_refuse() {
        assert!(est_plus_recente("2.0.75", "2.0.74"));
        assert!(est_plus_recente("2.1.0", "2.0.99"));
        assert!(est_plus_recente("3.0.0", "2.9.9"));

        // Égale ou antérieure : pas de bandeau.
        assert!(!est_plus_recente("2.0.74", "2.0.74"));
        assert!(!est_plus_recente("2.0.73", "2.0.74"));

        // `10` est après `9`, pas avant : la comparaison est numérique, pas
        // alphabétique. C'est l'erreur classique, et elle se produirait au
        // passage de 2.0.9 à 2.0.10.
        assert!(est_plus_recente("2.0.10", "2.0.9"));

        // Et tout ce qui n'est pas trois nombres est refusé, parce que ça vient
        // du réseau.
        for fantaisie in [
            "2.0", "2.0.0.1", "v2.0.75", "2.0.75-beta", "", "   ",
            "2.0.x", "99999999.0.0", "-1.0.0", "2. 0.75",
        ] {
            assert!(
                !est_plus_recente(fantaisie, "2.0.74"),
                "numéro accepté alors qu'il ne devrait pas : {fantaisie:?}"
            );
        }
    }

    /// L'adresse proposée au membre est filtrée avant de lui être montrée.
    #[test]
    fn seule_une_adresse_https_propre_est_proposee() {
        assert_eq!(
            page_acceptable("https://exemple.invalide/meter"),
            Some("https://exemple.invalide/meter".to_string())
        );
        for mauvaise in [
            "http://exemple.invalide/meter",
            "javascript:alert(1)",
            "file:///C:/Windows",
            "https://utilisateur:motdepasse@exemple.invalide/",
            "https://exemple.invalide/ meter",
        ] {
            assert_eq!(page_acceptable(mauvaise), None, "adresse acceptée : {mauvaise:?}");
        }
    }
}
