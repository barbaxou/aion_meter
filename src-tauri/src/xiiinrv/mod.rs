//! Partage de build vers le site de la guilde XIII NRV.
//!
//! Ajout de la guilde au meter A2Tools. Tout ce qui est ici est **désactivé tant
//! qu'aucun jeton n'est renseigné** : sans jeton, ce module ne lit rien, n'envoie
//! rien, et le meter se comporte exactement comme la version d'origine.
//!
//! COMMENT ÇA MARCHE
//!   `observer()` reçoit chaque paquet déjà découpé par A2Tools — une seule ligne
//!   ajoutée dans `parse_perfect_packet`. On n'y reconnaît que quatre paquets :
//!
//!     33 36   fiche personnelle : nom, serveur, niveau, Item Level, PV, PM
//!     11 56   inventaire complet : les objets équipés et leur enchantement
//!     56 36   Combat Power (trois petits paquets à l'entrée en jeu)
//!     00 90   Genus Insight : les cinq familles de pets et leurs effets
//!
//!   Tout le reste est ignoré. Aucun message de discussion, aucune position,
//!   aucun autre joueur : uniquement la fiche de son propre personnage.
//!
//! CE QU'ON N'ENVOIE PAS
//!   Aucun nom d'objet : le jeu ne les fait pas circuler, et le site n'en veut
//!   pas. On envoie l'identifiant officiel de l'objet, son emplacement et son
//!   enchantement. C'est le site qui décide de l'affichage.
//!
//! L'envoi part au plus une fois toutes les quinze minutes, et seulement si
//! quelque chose a changé depuis la dernière fois.

pub mod collecte;
pub mod envoi;

pub use collecte::observer;
pub use envoi::{demarrer, envoyer_maintenant, etat_partage};

/// Clés de réglage, rangées avec celles d'A2Tools dans settings.json.
pub const CLE_JETON: &str = "xiiinrv_token";
pub const CLE_ACTIF: &str = "xiiinrv_partage_build";
pub const CLE_URL: &str = "xiiinrv_url";

/// Adresse par défaut de la fonction de réception du site.
pub const URL_PAR_DEFAUT: &str =
    "https://xgtetaqzpnbyekezzxrz.supabase.co/functions/v1/meter-push";
