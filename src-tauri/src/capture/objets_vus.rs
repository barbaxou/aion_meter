//! Objets vus passer dans le trafic entrant, avec leur position (ajout XIII NRV).
//!
//! Le meter ne reçoit pas la liste du monde : l'ouverture de la carte en jeu
//! n'envoie rien (capture du 15/10/2026). Il reçoit en revanche, en clair, les
//! objets proches du joueur. La carte se remplit donc en jouant.
//!
//! Trois paquets, mesurés sur la capture du 15/10/2026 :
//!   - `34 36` : objet du décor ou nœud de récolte. Identifiant de modèle (u32)
//!     à l'octet 7, position (x, y, z en f32, centimètres) à l'octet 11. Le
//!     modèle se traduit par `envobjs.json` du site.
//!   - `1e 56` : objet éphémère créé (id u16 aux octets 3-4, position à l'octet 7).
//!   - `1f 56` : le même objet retiré. Les ids sont propres à une zone.
//!
//! Les positions restent **locales** : rien ici n'est envoyé au site et rien
//! n'entre dans l'Evidence Slice. Le module est dans `capture`, donc sans Tauri
//! ni système de fichiers (il doit compiler pour wasm32) ; la sauvegarde est
//! faite côté desktop.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Plafond d'objets gardés : au-delà, les moins récemment vus sont écartés.
pub const MAX_OBJETS: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Genre {
    /// Objet du décor ou nœud de récolte (`34 36`), conservé.
    Decor,
    /// Objet éphémère (`1e 56`), présent tant que le jeu ne l'a pas retiré.
    Ephemere,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjetVu {
    pub genre: Genre,
    /// Identifiant de modèle (0 pour un éphémère : le paquet n'en porte pas).
    pub modele: u32,
    /// Centimètres, comme les bornes des cartes.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Dernière observation, en millisecondes depuis l'epoch.
    pub vu_a: i64,
    pub fois_vu: u32,
}

/// Ce qu'un paquet annonce.
#[derive(Debug, Clone, PartialEq)]
pub enum Evenement {
    Decor { modele: u32, x: f32, y: f32, z: f32 },
    EphemereApparait { id: u16, x: f32, y: f32, z: f32 },
    EphemereDisparait { id: u16 },
    /// `23 36` : le joueur est téléporté, les ids de la zone ne valent plus rien.
    ChangementDeZone,
}

/// Position plausible : finie, et dans les bornes des cartes (±408 000 cm, avec
/// une marge). Rien d'autre n'est gardé : un décodage décalé donnerait des
/// valeurs absurdes, et mieux vaut ignorer un paquet que polluer la carte.
fn position_valide(x: f32, y: f32, z: f32) -> bool {
    x.is_finite()
        && y.is_finite()
        && z.is_finite()
        && x.abs() <= 450_000.0
        && y.abs() <= 450_000.0
        && z.abs() <= 1_000_000.0
}

fn lire_f32(b: &[u8], i: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Lit un paquet dont `charge` commence à l'opcode (sans l'en-tête de longueur).
pub fn decoder(charge: &[u8]) -> Option<Evenement> {
    match (*charge.first()?, *charge.get(1)?) {
        // Objet du décor : modèle u32 à l'octet 7, position à l'octet 11.
        (0x34, 0x36) => {
            let modele = u32::from_le_bytes(charge.get(7..11)?.try_into().ok()?);
            let (x, y, z) = (lire_f32(charge, 11)?, lire_f32(charge, 15)?, lire_f32(charge, 19)?);
            position_valide(x, y, z).then_some(Evenement::Decor { modele, x, y, z })
        }
        // Objet éphémère créé : id u16 aux octets 3-4, position à l'octet 7.
        (0x1e, 0x56) => {
            let id = u16::from_le_bytes(charge.get(3..5)?.try_into().ok()?);
            let (x, y, z) = (lire_f32(charge, 7)?, lire_f32(charge, 11)?, lire_f32(charge, 15)?);
            position_valide(x, y, z).then_some(Evenement::EphemereApparait { id, x, y, z })
        }
        // Le même objet retiré.
        (0x1f, 0x56) => {
            let id = u16::from_le_bytes(charge.get(3..5)?.try_into().ok()?);
            Some(Evenement::EphemereDisparait { id })
        }
        // Le joueur lui-même est téléporté (entité 0).
        (0x23, 0x36) => (*charge.get(2)? == 0x00).then_some(Evenement::ChangementDeZone),
        _ => None,
    }
}

#[derive(Default)]
struct Etat {
    /// Le conservé : clé = (genre, modèle, x et y au mètre).
    conserves: HashMap<(Genre, u32, i32, i32), ObjetVu>,
    /// Les éphémères présents, par id de la zone courante.
    ephemeres: HashMap<u16, ObjetVu>,
    modifie: bool,
}

fn etat() -> &'static Mutex<Etat> {
    static E: OnceLock<Mutex<Etat>> = OnceLock::new();
    E.get_or_init(|| Mutex::new(Etat::default()))
}

fn cle(o: &ObjetVu) -> (Genre, u32, i32, i32) {
    (o.genre, o.modele, (o.x / 100.0).round() as i32, (o.y / 100.0).round() as i32)
}

/// Ecarte les moins récemment vus au-delà du plafond.
fn limiter(e: &mut Etat) {
    if e.conserves.len() <= MAX_OBJETS {
        return;
    }
    let mut par_date: Vec<(i64, (Genre, u32, i32, i32))> = e.conserves.iter().map(|(k, o)| (o.vu_a, *k)).collect();
    par_date.sort_unstable_by_key(|(date, _)| *date);
    let en_trop = e.conserves.len() - MAX_OBJETS;
    for (_, k) in par_date.into_iter().take(en_trop) {
        e.conserves.remove(&k);
    }
}

fn appliquer(ev: Evenement, maintenant: i64) {
    let mut e = etat().lock();
    match ev {
        Evenement::Decor { modele, x, y, z } => {
            let o = ObjetVu { genre: Genre::Decor, modele, x, y, z, vu_a: maintenant, fois_vu: 1 };
            let k = cle(&o);
            match e.conserves.get_mut(&k) {
                Some(deja) => {
                    deja.vu_a = maintenant;
                    deja.fois_vu = deja.fois_vu.saturating_add(1);
                }
                None => {
                    e.conserves.insert(k, o);
                    limiter(&mut e);
                }
            }
            e.modifie = true;
        }
        Evenement::EphemereApparait { id, x, y, z } => {
            let o = ObjetVu { genre: Genre::Ephemere, modele: 0, x, y, z, vu_a: maintenant, fois_vu: 1 };
            e.ephemeres.insert(id, o);
        }
        Evenement::EphemereDisparait { id } => {
            e.ephemeres.remove(&id);
        }
        Evenement::ChangementDeZone => e.ephemeres.clear(),
    }
}

/// Point d'entrée du flux : `paquet` commence par son en-tête de longueur.
pub fn observer(paquet: &[u8]) {
    // Un rejeu de capture (ou la re-dérivation) ne doit pas verser ses objets
    // dans la carte de la personne : seul le trafic en direct compte.
    if crate::clock::en_rejeu() {
        return;
    }
    let longueur = super::stream_processor::read_varint(paquet, 0);
    if longueur.length <= 0 {
        return;
    }
    let Some(charge) = paquet.get(longueur.length as usize..) else {
        return;
    };
    if let Some(ev) = decoder(charge) {
        appliquer(ev, crate::clock::now_ms());
    }
}

/// Tout ce qui est connu : le conservé et l'éphémère présent.
pub fn instantane() -> Vec<ObjetVu> {
    let e = etat().lock();
    e.conserves.values().chain(e.ephemeres.values()).cloned().collect()
}

/// Recharge ce qui avait été sauvegardé (au démarrage).
pub fn charger(objets: Vec<ObjetVu>) {
    let mut e = etat().lock();
    for o in objets.into_iter().filter(|o| o.genre == Genre::Decor && position_valide(o.x, o.y, o.z)) {
        e.conserves.insert(cle(&o), o);
    }
    limiter(&mut e);
}

/// Vrai si l'état conservé a changé depuis le dernier appel ; remet à faux.
pub fn a_sauvegarder() -> bool {
    std::mem::take(&mut etat().lock().modifie)
}

/// Remet le drapeau « à sauvegarder » (après un enregistrement qui a échoué).
pub fn marquer_a_sauvegarder() {
    etat().lock().modifie = true;
}

/// Ce qu'il faut écrire sur disque : le conservé seulement, pas l'éphémère.
pub fn a_conserver() -> Vec<ObjetVu> {
    etat().lock().conserves.values().cloned().collect()
}

#[cfg(test)]
pub(crate) fn remettre_a_zero() {
    *etat().lock() = Etat::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Octets réels de la capture du 15/10/2026 (flux de jeu), à partir de l'opcode.
    const DECOR: &str = "3436cda2020400acd91300f937d74778d3944700c0a044000000000080ca41010101cd235f39a10100000000000000000000eccc9660070000";
    const EPHEMERE_3FA4: &str = "1e56013fa40000763dbe472897af47000090426482be47298eaf4700009842e2960000d32b3a00dd43de1f01000000022e0004000000ff089e231c3f";
    const RETRAIT_04CE: &str = "1f560104ce0000e0960000502d3a007ab103000000ff0800";

    fn octets(h: &str) -> Vec<u8> {
        (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect()
    }
    fn f(b: [u8; 4]) -> f32 {
        f32::from_le_bytes(b)
    }
    /// Un paquet tel qu'il arrive dans le flux : longueur (varint, +3) puis charge.
    fn avec_longueur(charge: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        let mut n = charge.len() + 1 + 3; // 1 octet de longueur, plus la règle « +3 »
        if n >= 0x80 {
            n += 1;
            v.push(((n & 0x7f) | 0x80) as u8);
            v.push((n >> 7) as u8);
        } else {
            v.push(n as u8);
        }
        v.extend_from_slice(charge);
        v
    }

    #[test]
    fn un_34_36_donne_le_modele_et_la_position() {
        let e = decoder(&octets(DECOR));
        assert_eq!(
            e,
            Some(Evenement::Decor {
                modele: 1_300_908,
                x: f([0xf9, 0x37, 0xd7, 0x47]),
                y: f([0x78, 0xd3, 0x94, 0x47]),
                z: f([0x00, 0xc0, 0xa0, 0x44]),
            })
        );
    }

    #[test]
    fn un_1e56_donne_l_id_et_la_position_et_un_1f56_retire_cet_id() {
        assert_eq!(
            decoder(&octets(EPHEMERE_3FA4)),
            Some(Evenement::EphemereApparait {
                id: 0xa43f,
                x: f([0x76, 0x3d, 0xbe, 0x47]),
                y: f([0x28, 0x97, 0xaf, 0x47]),
                z: 72.0,
            })
        );
        assert_eq!(decoder(&octets(RETRAIT_04CE)), Some(Evenement::EphemereDisparait { id: 0xce04 }));
    }

    #[test]
    fn un_23_36_signale_le_changement_de_zone_seulement_pour_soi() {
        // `23 36 00` : l'entité 0 est le joueur lui-même.
        assert_eq!(decoder(&[0x23, 0x36, 0x00, 0, 0, 0, 0, 0]), Some(Evenement::ChangementDeZone));
        assert_eq!(decoder(&[0x23, 0x36, 0x07, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn un_paquet_tronque_ou_aberrant_est_ignore() {
        let mut court = octets(DECOR);
        court.truncate(15);
        assert_eq!(decoder(&court), None);
        assert_eq!(decoder(&octets(EPHEMERE_3FA4)[..12]), None);
        // Une position hors du monde (NaN ou des milliards) n'est pas un objet.
        let mut fou = octets(DECOR);
        fou[11..15].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(decoder(&fou), None);
        let mut loin = octets(DECOR);
        loin[15..19].copy_from_slice(&9.0e9f32.to_le_bytes());
        assert_eq!(decoder(&loin), None);
        assert_eq!(decoder(&[]), None);
        assert_eq!(decoder(&[0x34]), None);
    }

    // L'état est global : les tests qui le touchent passent un à un.
    static VERROU: Mutex<()> = Mutex::new(());

    #[test]
    fn rejouer_le_meme_paquet_ne_double_pas_les_entrees() {
        let _g = VERROU.lock();
        remettre_a_zero();
        let p = avec_longueur(&octets(DECOR));
        observer(&p);
        observer(&p);
        let tout = instantane();
        assert_eq!(tout.len(), 1);
        assert_eq!(tout[0].genre, Genre::Decor);
        assert_eq!(tout[0].modele, 1_300_908);
        assert_eq!(tout[0].fois_vu, 2);
        assert!(a_sauvegarder());
        assert!(!a_sauvegarder(), "le drapeau se remet à faux");
    }

    #[test]
    fn un_ephemere_apparait_puis_disparait_et_ne_se_sauvegarde_pas() {
        let _g = VERROU.lock();
        remettre_a_zero();
        observer(&avec_longueur(&octets(EPHEMERE_3FA4)));
        assert_eq!(instantane().iter().filter(|o| o.genre == Genre::Ephemere).count(), 1);
        assert!(a_conserver().is_empty());
        // Le retrait porte le même id que la création.
        let mut retrait = octets(RETRAIT_04CE);
        retrait[3] = 0x3f;
        retrait[4] = 0xa4;
        observer(&avec_longueur(&retrait));
        assert_eq!(instantane().iter().filter(|o| o.genre == Genre::Ephemere).count(), 0);
    }

    #[test]
    fn un_changement_de_zone_vide_les_ephemeres_mais_garde_le_decor() {
        let _g = VERROU.lock();
        remettre_a_zero();
        observer(&avec_longueur(&octets(DECOR)));
        observer(&avec_longueur(&octets(EPHEMERE_3FA4)));
        observer(&avec_longueur(&[0x23, 0x36, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        let tout = instantane();
        assert_eq!(tout.len(), 1);
        assert_eq!(tout[0].genre, Genre::Decor);
    }

    #[test]
    fn un_rejeu_de_capture_ne_remplit_pas_la_carte() {
        let _g = VERROU.lock();
        remettre_a_zero();
        crate::clock::set_override(Some(1_700_000_000_000));
        observer(&avec_longueur(&octets(DECOR)));
        crate::clock::set_override(None);
        assert!(instantane().is_empty(), "le trafic rejoué ne doit pas être gardé");
        assert!(!a_sauvegarder());
    }

    #[test]
    fn le_plafond_ecarte_les_plus_anciens() {
        let _g = VERROU.lock();
        remettre_a_zero();
        let mut base = Vec::new();
        for i in 0..(MAX_OBJETS as i64 + 50) {
            base.push(ObjetVu { genre: Genre::Decor, modele: 1, x: ((i % 2000) * 200) as f32, y: (i / 2000 * 200) as f32, z: 0.0, vu_a: i, fois_vu: 1 });
        }
        charger(base);
        let gardes = a_conserver();
        assert_eq!(gardes.len(), MAX_OBJETS);
        assert!(gardes.iter().all(|o| o.vu_a >= 50), "les 50 plus anciens doivent partir");
    }
}
