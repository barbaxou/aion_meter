//! Le jeton de la guilde, scellé plutôt qu'écrit en clair.
//!
//! Relevé pendant l'audit du 08/10/2026 : le jeton était écrit en clair à
//! **deux** endroits, pas un seul.
//!
//! 1. `%APPDATA%\gg.xiiinrv.meter\settings.json` ;
//! 2. `%LOCALAPPDATA%\gg.xiiinrv.meter\EBWebView\…\Local Storage\leveldb`,
//!    parce que `setSetting` dans le pont écrit aussi dans `localStorage`.
//!
//! Le second était le plus discret, et c'est celui qu'on oublie. Le jeton passe
//! donc désormais par ses propres commandes, qui ne touchent jamais au
//! `localStorage` de la page, et il est scellé par DPAPI — chiffré pour le
//! compte Windows courant, sur cette machine-là. Copier le fichier ailleurs ne
//! donne rien.
//!
//! **Ce que ça protège** : un fichier copié, une sauvegarde, un profil
//! exfiltré. **Ce que ça ne protège pas** : un programme qui tourne sous le
//! compte du membre, qui peut demander à Windows de déchiffrer exactement comme
//! le meter le fait. C'est la limite de DPAPI, et elle est assumée.
//!
//! Sur une plateforme sans magasin à secrets (Linux), le jeton reste en clair :
//! refuser l'enregistrement couperait le partage pour ces membres, ce qui est
//! pire que le risque évité. Le journal le dit au démarrage.

use crate::platform::secret;

/// Ce qui précède un jeton scellé dans `settings.json`.
///
/// Sa présence distingue un jeton scellé d'un jeton en clair, donc d'une
/// installation plus ancienne qu'il faut reprendre. Un jeton de la guilde est
/// hexadécimal, donc il ne peut pas commencer par ces caractères-là.
const PREFIXE: &str = "dpapi:";

/// Le sel lié à cet usage. DPAPI le réclame à l'ouverture comme au scellement,
/// donc un blob scellé pour le jeton ne s'ouvre pas pour autre chose.
const SEL: &[u8] = b"xiiinrv.jeton.v1";

fn en_hexa(octets: &[u8]) -> String {
    let mut s = String::with_capacity(octets.len() * 2);
    for o in octets {
        s.push_str(&format!("{o:02x}"));
    }
    s
}

fn depuis_hexa(texte: &str) -> Option<Vec<u8>> {
    if texte.len() % 2 != 0 {
        return None;
    }
    (0..texte.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&texte[i..i + 2], 16).ok())
        .collect()
}

/// Scelle un jeton pour l'écrire dans les réglages.
///
/// Rend le jeton tel quel si la plateforme n'a pas de magasin à secrets : mieux
/// vaut un partage qui marche qu'un partage impossible.
pub fn sceller(jeton: &str) -> String {
    let jeton = jeton.trim();
    if jeton.is_empty() {
        return String::new();
    }
    match secret::protect(jeton.as_bytes(), SEL) {
        Some(scelle) => format!("{PREFIXE}{}", en_hexa(&scelle)),
        None => {
            tracing::warn!(
                "XIII NRV : pas de magasin à secrets sur cette plateforme, \
                 le jeton est conservé en clair"
            );
            jeton.to_string()
        }
    }
}

/// Ouvre ce qui a été lu dans les réglages.
///
/// Accepte un jeton en clair : c'est ce qu'écrivaient les versions antérieures
/// à 2.0.75, et il ne faut pas que la mise à jour coupe le partage.
pub fn ouvrir(enregistre: &str) -> Option<String> {
    let enregistre = enregistre.trim();
    if enregistre.is_empty() {
        return None;
    }
    let Some(hexa) = enregistre.strip_prefix(PREFIXE) else {
        return Some(enregistre.to_string());
    };
    let octets = depuis_hexa(hexa)?;
    let ouvert = secret::unprotect(&octets, SEL)?;
    String::from_utf8(ouvert).ok()
}

/// Ce jeton enregistré a-t-il besoin d'être repris ?
///
/// Vrai quand il est en clair alors que la plateforme sait le sceller : c'est
/// une installation d'avant 2.0.75, et il faut la reprendre une fois.
pub fn a_resceller(enregistre: &str) -> bool {
    !enregistre.trim().is_empty()
        && !enregistre.trim_start().starts_with(PREFIXE)
        && secret::available()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un jeton scellé ne se lit pas dans le fichier, et se rouvre ici.
    ///
    /// Sur une plateforme sans magasin à secrets, le test vérifie l'autre
    /// moitié du contrat : le jeton reste utilisable.
    #[test]
    fn un_jeton_scelle_se_rouvre_et_ne_se_lit_pas_en_clair() {
        let jeton = "00000000DEMOJETONDETEST00000000000000000000000000000000000000000";
        let enregistre = sceller(jeton);
        assert_eq!(ouvrir(&enregistre).as_deref(), Some(jeton), "il doit se rouvrir");

        if crate::platform::secret::available() {
            assert!(
                !enregistre.contains(jeton),
                "le jeton ne doit pas apparaître tel quel dans ce qui est écrit"
            );
            assert!(enregistre.starts_with(PREFIXE));
            assert!(!a_resceller(&enregistre), "déjà scellé, rien à reprendre");
        } else {
            assert_eq!(enregistre, jeton, "sans magasin à secrets, on garde le jeton");
        }
    }

    /// Une installation antérieure garde un jeton en clair : il doit continuer
    /// de fonctionner, et être signalé comme à reprendre.
    #[test]
    fn un_jeton_en_clair_dune_ancienne_version_marche_encore() {
        let jeton = "abc123";
        assert_eq!(ouvrir(jeton).as_deref(), Some(jeton));
        assert_eq!(a_resceller(jeton), crate::platform::secret::available());
    }

    /// Et rien ne sort de rien.
    #[test]
    fn sans_jeton_il_ny_a_rien_a_ouvrir() {
        assert_eq!(sceller(""), "");
        assert_eq!(sceller("   "), "");
        assert_eq!(ouvrir(""), None);
        assert_eq!(ouvrir("   "), None);
        assert!(!a_resceller(""));
        // Un scellé illisible — fichier tronqué, ou copié depuis une autre
        // machine — ne doit pas faire tomber le meter.
        assert_eq!(ouvrir("dpapi:zz"), None);
        assert_eq!(ouvrir("dpapi:00ff"), None);
    }
}
