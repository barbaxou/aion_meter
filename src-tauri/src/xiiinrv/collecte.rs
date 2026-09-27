//! Lecture des quatre paquets qui nous intéressent.
//!
//! Portage direct de nos décodeurs Python (`tools/meter/` du dépôt du site),
//! vérifiés sur deux sessions réelles : 27 objets sur 27, 5 familles de pets et
//! leurs 35 effets, niveau 45, Item Level 3009, Combat Power 132 462, PV 25 835,
//! PM 5 678 — tous identiques à ce que le jeu affiche.

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Emplacements retenus : équipement, runes (23-24), arcanes (41-45).
fn emplacement_valide(e: u8) -> bool {
    (1..=24).contains(&e) || (41..=45).contains(&e)
}

const CONTENEUR_EQUIPE: u8 = 0x0B;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Piece {
    pub emplacement: u8,
    #[serde(rename = "itemId")]
    pub item_id: u32,
    pub enchantement: u8,
    pub conteneur: u8,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Effet {
    pub slot: u8,
    pub rarete: u8,
    pub stat: u16,
    pub valeur: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Genus {
    pub genus: &'static str,
    pub niveau: u32,
    pub xp: u32,
    pub effets: Vec<Effet>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Etat {
    pub nom: Option<String>,
    pub serveur: Option<String>,
    pub niveau: Option<u32>,
    pub item_level: Option<u32>,
    pub pv: Option<u32>,
    pub pm: Option<u32>,
    pub combat_power: Option<u32>,
    pub equipement: Vec<Piece>,
    pub pets: Vec<Genus>,
}

impl Etat {
    /// Y a-t-il de quoi envoyer ? Le nom du personnage est indispensable : c'est
    /// lui qui relie la fiche au Roster du site.
    pub fn pret(&self) -> bool {
        self.nom.is_some() && (!self.equipement.is_empty() || self.item_level.is_some())
    }
}

/// Interrupteur de lecture, **fermé par défaut**. Il n'est ouvert que lorsqu'un
/// jeton est enregistré ET que le partage est coché. Tant qu'il est fermé,
/// `observer()` ressort immédiatement : aucun paquet n'est analysé, rien n'est
/// gardé en mémoire, et le meter se comporte exactement comme la version
/// d'origine d'A2Tools.
static LECTURE_OUVERTE: AtomicBool = AtomicBool::new(false);

/// Appelé par `envoi::configurer()` à chaque changement de réglage.
pub fn ouvrir_lecture(ouverte: bool) {
    let avant = LECTURE_OUVERTE.swap(ouverte, Ordering::Relaxed);
    if avant && !ouverte {
        // On vient de refermer : on n'a aucune raison de garder la fiche,
        // ni de continuer à laisser passer un flux.
        vider();
        flux_retenus().lock().clear();
    }
}

pub fn lecture_ouverte() -> bool {
    LECTURE_OUVERTE.load(Ordering::Relaxed)
}

/// Nom du personnage tel qu'A2Tools le détecte de son côté.
///
/// La fiche `33 36` n'est envoyée qu'à l'entrée en jeu et aux changements de
/// zone : si le partage est activé après ce moment, on ne la verra pas de toute
/// la session. A2Tools, lui, retrouve le nom autrement. On s'en sert comme
/// filet : sans lui, une fiche remontée sans nom ne pourrait pas être reliée au
/// Roster du site.
static NOM_DETECTE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn nom_detecte_stock() -> &'static Mutex<Option<String>> {
    NOM_DETECTE.get_or_init(|| Mutex::new(None))
}

pub fn nom_detecte(nom: Option<String>) {
    let nom = nom.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    *nom_detecte_stock().lock() = nom;
}

static ETAT: OnceLock<Mutex<Etat>> = OnceLock::new();

fn etat() -> &'static Mutex<Etat> {
    ETAT.get_or_init(|| Mutex::new(Etat::default()))
}

pub fn lire_etat() -> Etat {
    let mut e = etat().lock().clone();
    if e.nom.is_none() {
        e.nom = nom_detecte_stock().lock().clone();
    }
    e
}

pub fn vider() {
    *etat().lock() = Etat::default();
}

// ---------------------------------------------------------------------------
// Lecture bas niveau
// ---------------------------------------------------------------------------

fn u16_le(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_le(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Entier à longueur variable, comme dans le reste du protocole.
/// Renvoie (valeur, nombre d'octets lus).
fn varint(d: &[u8], o: usize) -> Option<(u32, usize)> {
    let mut valeur: u32 = 0;
    let mut decalage = 0;
    let mut lus = 0;
    loop {
        let octet = *d.get(o + lus)?;
        lus += 1;
        valeur |= ((octet & 0x7F) as u32) << decalage;
        if octet & 0x80 == 0 {
            return Some((valeur, lus));
        }
        decalage += 7;
        if decalage >= 32 {
            return None;
        }
    }
}

/// Début du contenu d'un paquet : on saute la longueur (varint) et l'opcode.
/// Renvoie (opcode sur 2 octets, position juste après).
fn entete(packet: &[u8]) -> Option<([u8; 2], usize)> {
    let (_, lus) = varint(packet, 0)?;
    let a = *packet.get(lus)?;
    let b = *packet.get(lus + 1)?;
    Some(([a, b], lus + 2))
}

// ---------------------------------------------------------------------------
// Point d'entrée : un paquet, déjà découpé par A2Tools
// ---------------------------------------------------------------------------

/// Faut-il laisser passer ce trafic avant que le port de combat ne soit
/// verrouillé ?
///
/// Tant que le port n'est pas verrouillé, A2Tools écarte tout ce qui ne
/// ressemble pas à du combat. Or la fiche de personnage, l'inventaire et le
/// Combat Power arrivent précisément à ce moment, à l'entrée en jeu.
///
/// Deux essais avant celui-ci :
///   1. ne laisser passer que les morceaux contenant l'un de nos opcodes —
///      insuffisant : un paquet de plusieurs kilo-octets est découpé par le
///      réseau et seul le premier morceau porte l'opcode, les suivants étaient
///      jetés et le paquet ne pouvait plus être reconstitué ;
///   2. tout laisser passer — trop large : le meter ne reconnaissait plus le
///      flux du jeu et ne détectait plus les combats.
///
/// D'où cette version : dès qu'un de nos paquets est vu sur un flux, **ce
/// flux-là** est retenu et passe entièrement. Les autres flux restent filtrés
/// comme avant, donc la détection du combat n'est pas touchée.
pub fn interesse(port_a: u16, port_b: u16, donnees: &[u8]) -> bool {
    if !lecture_ouverte() {
        return false;
    }
    let cle = (port_a.min(port_b), port_a.max(port_b));
    if flux_retenus().lock().contains(&cle) {
        return true;
    }

    const CIBLES: [[u8; 2]; 4] = [[0x33, 0x36], [0x11, 0x56], [0x56, 0x36], [0x00, 0x90]];
    let vu = donnees
        .windows(2)
        .any(|f| CIBLES.iter().any(|c| f[0] == c[0] && f[1] == c[1]));
    if vu {
        let mut liste = flux_retenus().lock();
        if !liste.contains(&cle) {
            liste.push(cle);
        }
    }
    vu
}

static FLUX_RETENUS: OnceLock<Mutex<Vec<(u16, u16)>>> = OnceLock::new();

fn flux_retenus() -> &'static Mutex<Vec<(u16, u16)>> {
    FLUX_RETENUS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn observer(packet: &[u8]) {
    // Rien n'est lu tant qu'aucun jeton n'est enregistré et que le partage n'est
    // pas coché. C'est le tout premier test, avant même de regarder le paquet.
    if !lecture_ouverte() {
        return;
    }
    let Some((opcode, _apres)) = entete(packet) else {
        return;
    };
    match opcode {
        [0x33, 0x36] => lire_fiche(packet),
        [0x11, 0x56] => lire_equipement(packet),
        [0x56, 0x36] => lire_combat_power(packet),
        [0x00, 0x90] => lire_pets(packet),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 33 36 — fiche personnelle
// ---------------------------------------------------------------------------

/// `<nom UTF-8> <serveur u16> <classe u32> <1 octet> <niveau u32> <Item Level u32>`,
/// et plus loin les PV en varint répété deux fois puis les PM en u32 répétés.
///
/// Le nom n'est pas précédé d'un marqueur fiable : on le repère comme la première
/// suite d'au moins trois caractères latins suivie d'un niveau et d'un Item Level
/// plausibles. Une fois trouvé, il ne bouge plus.
fn lire_fiche(packet: &[u8]) {
    let mut trouve: Option<(String, u16, u32, u32)> = None;

    for debut in 0..packet.len() {
        let mut fin = debut;
        while fin < packet.len() && est_lettre(packet[fin]) {
            fin += 1;
        }
        let longueur = fin - debut;
        if !(3..=20).contains(&longueur) {
            continue;
        }
        // Après le nom : serveur (u16), classe (u32), un octet, niveau, Item Level.
        let (Some(serveur), Some(niveau), Some(item_level)) = (
            u16_le(packet, fin),
            u32_le(packet, fin + 7),
            u32_le(packet, fin + 11),
        ) else {
            continue;
        };
        if !(1..=200).contains(&niveau) || !(1..=100_000).contains(&item_level) {
            continue;
        }
        if let Ok(nom) = std::str::from_utf8(&packet[debut..fin]) {
            trouve = Some((nom.to_string(), serveur, niveau, item_level));
            break;
        }
    }

    let Some((nom, serveur, niveau, item_level)) = trouve else {
        return;
    };
    let (pv, pm) = lire_pv_pm(packet);

    let mut e = etat().lock();
    e.nom = Some(nom);
    e.serveur = Some(nom_du_serveur(serveur));
    e.niveau = Some(niveau);
    e.item_level = Some(item_level);
    if pv.is_some() {
        e.pv = pv;
        e.pm = pm;
    }
}

fn est_lettre(o: u8) -> bool {
    o.is_ascii_alphabetic()
}

fn nom_du_serveur(id: u16) -> String {
    match id {
        1015 => "Kasaka (TW)".to_string(),
        autre => format!("Serveur {}", autre),
    }
}

/// PV (varint, répété deux fois : actuels et maximum) puis PM (u32 répétés).
/// C'est ce couple répété qui sert de signature — et il donne le **total**
/// affiché en jeu, pas la valeur de base de la fiche de statistiques.
fn lire_pv_pm(packet: &[u8]) -> (Option<u32>, Option<u32>) {
    let mut i = 8;
    while i + 8 <= packet.len() {
        let (Some(pm1), Some(pm2)) = (u32_le(packet, i), u32_le(packet, i + 4)) else {
            break;
        };
        if pm1 == pm2 && (500..200_000).contains(&pm1) {
            for taille in [4usize, 6, 8] {
                if taille > i {
                    continue;
                }
                let Some((pv1, l1)) = varint(packet, i - taille) else {
                    continue;
                };
                let Some((pv2, l2)) = varint(packet, i - taille + l1) else {
                    continue;
                };
                if pv1 == pv2 && l1 + l2 == taille && (1_000..1_000_000).contains(&pv1) {
                    return (Some(pv1), Some(pm1));
                }
            }
        }
        i += 1;
    }
    (None, None)
}

// ---------------------------------------------------------------------------
// 11 56 — inventaire complet : les objets équipés
// ---------------------------------------------------------------------------

/// Un objet équipé se reconnaît à sa seule forme, sans aucune base extérieure :
/// identifiant de 9 chiffres commençant par 1, 2, 3 ou 8 (la famille), conteneur
/// `0x0B` douze octets plus loin, emplacement juste après. L'enchantement est le
/// premier octet non nul des seize octets suivants.
fn lire_equipement(packet: &[u8]) {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut vus: Vec<u8> = Vec::new();

    let mut o = 0usize;
    while o + 14 < packet.len() {
        let Some(item_id) = u32_le(packet, o) else {
            break;
        };
        let premier_chiffre = item_id / 100_000_000;
        let forme_valide = (100_000_000..=899_999_999).contains(&item_id)
            && matches!(premier_chiffre, 1 | 2 | 3 | 8);

        if forme_valide && packet[o + 12] == CONTENEUR_EQUIPE && emplacement_valide(packet[o + 13])
        {
            let emplacement = packet[o + 13];
            if !vus.contains(&emplacement) {
                vus.push(emplacement);
                let fin = (o + 30).min(packet.len());
                let enchantement = packet[o + 14..fin].iter().copied().find(|b| *b != 0).unwrap_or(0);
                pieces.push(Piece {
                    emplacement,
                    item_id,
                    enchantement,
                    conteneur: CONTENEUR_EQUIPE,
                });
            }
        }
        o += 1;
    }

    if pieces.is_empty() {
        return;
    }
    pieces.sort_by_key(|p| p.emplacement);
    etat().lock().equipement = pieces;
}

// ---------------------------------------------------------------------------
// 56 36 — Combat Power
// ---------------------------------------------------------------------------

/// Trois petits paquets de 19 octets font défiler le compteur à l'entrée en jeu
/// (77 148 → 111 394 → 132 462) : on garde simplement la dernière valeur reçue.
fn lire_combat_power(packet: &[u8]) {
    if packet.len() != 19 {
        return;
    }
    let Some(valeur) = u32_le(packet, 3) else {
        return;
    };
    if (1_000..100_000_000).contains(&valeur) {
        etat().lock().combat_power = Some(valeur);
    }
}

// ---------------------------------------------------------------------------
// 00 90 — Genus Insight (les pets)
// ---------------------------------------------------------------------------

const GENUS: [(u8, &str); 5] = [
    (2, "Cogni"),
    (3, "Fera"),
    (4, "Natura"),
    (5, "Varian"),
    (6, "Special"),
];
/// Un octet 0xAA revient toutes les ~50 octets dans cette zone (marqueur de
/// découpage du flux) : on le retire avant de lire.
const MARQUEUR: u8 = 0xAA;

fn lire_pets(packet: &[u8]) {
    let mut familles: Vec<Genus> = Vec::new();

    for (id, nom) in GENUS {
        let Some(debut) = trouver_entete_genus(packet, id) else {
            continue;
        };
        let (Some(niveau), Some(xp)) = (u32_le(packet, debut + 2), u32_le(packet, debut + 6)) else {
            continue;
        };

        // 4 octets à 0, puis `03 01 <niveau u8>`, puis les enregistrements.
        let zone: Vec<u8> = packet[(debut + 10).min(packet.len())..]
            .iter()
            .copied()
            .filter(|o| *o != MARQUEUR)
            .take(340)
            .collect();

        let mut effets = Vec::new();
        let mut o = 7usize;
        let mut attendu = 0u8;
        while o + 12 <= zone.len() {
            let slot = zone[o];
            let rarete = zone[o + 1];
            let (Some(stat), Some(valeur), Some(pad)) = (
                u16_le(&zone, o + 2),
                u32_le(&zone, o + 4),
                u32_le(&zone, o + 8),
            ) else {
                break;
            };
            if slot != attendu || rarete > 5 || stat > 1023 || pad != 0 {
                break;
            }
            if rarete > 0 && valeur > 0 {
                effets.push(Effet {
                    slot,
                    rarete,
                    stat,
                    valeur,
                });
            }
            o += 12;
            attendu = if slot < 6 { slot + 1 } else { 0 };
        }

        if !effets.is_empty() {
            familles.push(Genus {
                genus: nom,
                niveau,
                xp,
                effets,
            });
        }
    }

    if !familles.is_empty() {
        etat().lock().pets = familles;
    }
}

/// En-tête d'une famille : `<id><id><niveau u32><xp u32>`, avec des valeurs
/// plausibles — c'est ce qui évite de tomber sur deux octets identiques au hasard.
fn trouver_entete_genus(packet: &[u8], id: u8) -> Option<usize> {
    let mut o = 0usize;
    while o + 10 <= packet.len() {
        if packet[o] == id && packet[o + 1] == id {
            if let (Some(niveau), Some(xp)) = (u32_le(packet, o + 2), u32_le(packet, o + 6)) {
                if (1..=30).contains(&niveau) && (1..=2_000_000).contains(&xp) {
                    return Some(o);
                }
            }
        }
        o += 1;
    }
    None
}
