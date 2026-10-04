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
//! ```text
//!   33 36   fiche personnelle : nom, serveur, niveau, Item Level, PV, PM
//!   11 56   inventaire complet : les objets équipés et leur enchantement
//!   56 36   Combat Power (trois petits paquets à l'entrée en jeu)
//!   00 90   Genus Insight : les cinq familles de pets et leurs effets
//! ```
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

/// Clé publique du projet Supabase.
///
/// Ce n'est pas un secret : elle est servie à chaque visiteur du site, dans
/// `app.js`. Elle ne donne aucun droit par elle-même — nos tables refusent
/// toute écriture venant d'un navigateur, et la fonction de réception n'est
/// ouverte qu'à la clé de service, qui reste côté serveur.
///
/// On l'envoie parce que Supabase peut exiger un jeton d'accès sur ses
/// fonctions Edge selon la façon dont elles ont été déployées. La vraie
/// authentification reste le jeton personnel du membre, vérifié en base.
pub const CLE_PUBLIQUE: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InhndGV0YXF6cG5ieWVrZXp6eHJ6Iiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODQ5ODQ1NDcsImV4cCI6MjEwMDU2MDU0N30.IEkoBiXmChGRoUH3WYtIniISP2YHY4B40IX3s0tI6KE";

/// Adresse par défaut de la fonction de réception du site.
pub const URL_PAR_DEFAUT: &str =
    "https://xgtetaqzpnbyekezzxrz.supabase.co/functions/v1/meter-push";
