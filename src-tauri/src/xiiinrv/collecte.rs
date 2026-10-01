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
use tracing::info;

/// Emplacements retenus : équipement, runes (23-24), arcanes (41-45).
fn emplacement_valide(e: u8) -> bool {
    (1..=24).contains(&e) || (41..=45).contains(&e)
}

const CONTENEUR_EQUIPE: u8 = 0x0B;

/// Deux lectures séparées de moins que ça font partie de la même entrée en jeu.
/// Mesuré le 29/09 : Combat Power, inventaire, pets et fiche arrivent en neuf
/// secondes. Une minute laisse de la marge sans jamais rapprocher deux entrées
/// en jeu différentes.
const MEME_ENTREE_EN_JEU: std::time::Duration = std::time::Duration::from_secs(60);

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
    /// Quand la dernière valeur de Combat Power est arrivée : sert à distinguer
    /// le défilement de l'entrée en jeu d'un vrai changement.
    pub cp_vu_le: Option<std::time::Instant>,
    /// Combien de paquets de Combat Power composent le défilement en cours.
    /// Un défilement authentique en compte trois ; un paquet isolé est suspect.
    pub cp_paquets: u8,
    /// La plus grande valeur du défilement en cours, retenue tant qu'elle n'est
    /// pas corroborée par un second paquet.
    pub cp_en_attente: Option<u32>,
    pub equipement: Vec<Piece>,
    pub pets: Vec<Genus>,
    /// Quand l'équipement et les pets ont été lus. À l'entrée en jeu ils
    /// arrivent **avant** le nom — huit secondes avant, mesuré le 29/09 — donc
    /// sans ces dates on ne saurait pas, au changement de personnage, lesquels
    /// appartiennent au nouveau et lesquels à l'ancien.
    pub equipement_vu_le: Option<std::time::Instant>,
    pub pets_vu_le: Option<std::time::Instant>,
}

impl Etat {
    /// Y a-t-il de quoi envoyer **automatiquement** ?
    ///
    /// Le nom est indispensable — c'est lui qui relie la fiche au Roster. Et on
    /// attend l'équipement : la fiche de personnage et l'inventaire n'arrivent
    /// pas au même instant, et partir dès la fiche lue envoyait un personnage
    /// sans une seule pièce.
    pub fn pret(&self) -> bool {
        self.nom.is_some() && !self.equipement.is_empty()
    }

    /// Un autre personnage vient d'entrer en jeu : jeter ce qui appartenait au
    /// précédent.
    ///
    /// À l'entrée en jeu le nom arrive **en dernier** — huit secondes après
    /// l'équipement, mesuré le 29/09/2026. Entre les deux, la fiche porte
    /// l'équipement du nouveau personnage et le nom de l'ancien, et un envoi
    /// tombant là attribuerait le stuff d'un personnage à un autre. Tout ce qui
    /// a été lu peu avant ce nom fait donc partie de la même entrée en jeu et se
    /// garde ; le reste s'efface.
    ///
    /// Prend l'heure en paramètre plutôt que de la lire : c'est ce qui permet de
    /// vérifier les deux cas sans attendre une minute.
    pub fn changer_de_personnage(&mut self, maintenant: std::time::Instant) {
        let meme_entree = |quand: Option<std::time::Instant>| {
            quand.is_some_and(|t| {
                maintenant.checked_duration_since(t).is_some_and(|age| age < MEME_ENTREE_EN_JEU)
            })
        };
        if !meme_entree(self.equipement_vu_le) {
            self.equipement.clear();
            self.equipement_vu_le = None;
        }
        if !meme_entree(self.pets_vu_le) {
            self.pets.clear();
            self.pets_vu_le = None;
        }
        if !meme_entree(self.cp_vu_le) {
            self.combat_power = None;
            self.cp_vu_le = None;
            self.cp_paquets = 0;
            self.cp_en_attente = None;
        }
        // Les PV et PM arrivent dans la fiche elle-même, mais pas dans toutes :
        // la version imbriquée de l'entrée en jeu ne les porte pas, d'où le
        // « on n'écrase que si la fiche en contient » de `lire_fiche`. Au
        // changement de personnage, cette prudence se retourne contre nous : elle
        // laisserait les PV d'un autre. On les efface, la fiche qui suit les
        // remplira.
        self.pv = None;
        self.pm = None;
    }

    /// Y a-t-il quelque chose à envoyer quand on clique soi-même sur le bouton ?
    /// Plus permissif : si le membre insiste, on envoie ce qu'on a.
    pub fn envoyable(&self) -> bool {
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
        super::tampon::oublier();
    }
}

pub fn lecture_ouverte() -> bool {
    LECTURE_OUVERTE.load(Ordering::Relaxed)
}

/// Nom du personnage tel qu'A2Tools le détecte de son côté.
///
/// La fiche `33 36` n'est pas envoyée à l'entrée en jeu : elle arrive quand le
/// joueur ouvre l'écran Pets › Genus Insight, à la milliseconde près en même
/// temps que le paquet des pets (constaté sur deux sessions enregistrées).
/// Tant qu'il ne l'a pas ouvert, on n'a pas son nom. A2Tools, lui, le retrouve
/// autrement. On s'en sert comme
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

/// Le stockage d'A2Tools, pour y lire la composition du groupe.
///
/// Notre lecture des paquets reste la nôtre — c'est leur réassemblage qui
/// perdait des paquets, pas leurs décodeurs. Celui de la composition du groupe
/// fait une centaine de lignes avec des ancrages délicats (la distance entre
/// l'Item Level et le Combat Power n'y est pas fixe) : le réécrire serait
/// fragile pour rien. On réutilise son résultat, et s'il manque on ne rafraîchit
/// simplement pas.
static GROUPE: OnceLock<std::sync::Arc<crate::combat::data_storage::DataStorage>> =
    OnceLock::new();

pub fn brancher_le_groupe(stockage: std::sync::Arc<crate::combat::data_storage::DataStorage>) {
    let _ = GROUPE.set(stockage);
}

/// Reprend l'Item Level et le Combat Power dans la ligne du membre lui-même.
///
/// L'inventaire et le défilement du Combat Power n'arrivent qu'à l'entrée en
/// jeu : sans cela, une fiche reste celle du moment où le membre est entré, et
/// un changement d'équipement en cours de soirée ne se voit pas. La composition
/// du groupe, elle, passe environ toutes les vingt secondes — 1 703 fois sur la
/// session de dix heures du 30/09/2026.
///
/// **On n'y prend que la ligne du membre.** Les autres joueurs y figurent aussi,
/// et on n'y touche pas : tout le dispositif repose sur le fait que chacun
/// partage sa propre fiche, et la notice remise aux membres le dit.
pub fn rafraichir_depuis_le_groupe() {
    let Some(stockage) = GROUPE.get() else {
        return;
    };
    if !lecture_ouverte() {
        return;
    }
    let nom = {
        let e = etat().lock();
        match e.nom.clone() {
            Some(n) => n,
            None => nom_detecte_stock().lock().clone().unwrap_or_default(),
        }
    };
    if nom.is_empty() {
        return;
    }
    let membres = stockage.get_party_members();
    let Some(moi) = membres.get(&nom) else {
        return; // hors groupe, ou nom pas encore connu
    };

    let mut e = etat().lock();
    let mut change = false;
    if moi.gear_score > 0 && e.item_level != Some(moi.gear_score as u32) {
        e.item_level = Some(moi.gear_score as u32);
        change = true;
    }
    if moi.combat_power > 0 && e.combat_power != Some(moi.combat_power as u32) {
        e.combat_power = Some(moi.combat_power as u32);
        // Cette valeur est structurée et nommée : elle vaut mieux qu'un
        // défilement, donc elle compte comme corroborée.
        e.cp_paquets = 2;
        e.cp_en_attente = Some(moi.combat_power as u32);
        e.cp_vu_le = Some(std::time::Instant::now());
        change = true;
    }
    if moi.level > 0 && e.niveau != Some(moi.level as u32) {
        e.niveau = Some(moi.level as u32);
        change = true;
    }
    if change {
        info!(
            "XIII NRV : fiche rafraîchie depuis le groupe (niveau {:?}, Item Level {:?}, Combat Power {:?})",
            e.niveau, e.item_level, e.combat_power
        );
    }
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

pub fn observer(packet: &[u8]) {
    // Rien n'est lu tant qu'aucun jeton n'est enregistré et que le partage n'est
    // pas coché. C'est le tout premier test, avant même de regarder le paquet.
    if !lecture_ouverte() {
        return;
    }
    let Some((opcode, _apres)) = entete(packet) else {
        return;
    };
    // La fiche de personnage n'arrive pas toujours seule : à l'entrée en jeu
    // elle est imbriquée dans un plus gros paquet (`60 88`), et notre lecture,
    // qui ne regardait que l'opcode extérieur, passait à côté. On la cherche
    // donc dans tout paquet qui porte la marque `33 36`, tant qu'on n'a pas
    // encore de niveau.
    if opcode != [0x33, 0x36] && etat().lock().niveau.is_none() {
        let porte_la_marque = packet
            .windows(2)
            .any(|f| f[0] == 0x33 && f[1] == 0x36);
        if porte_la_marque {
            lire_fiche(packet);
        }
    }

    match opcode {
        [0x33, 0x36] => {
            info!("XIII NRV : fiche de personnage vue ({} octets)", packet.len());
            lire_fiche(packet)
        }
        [0x11, 0x56] => {
            info!("XIII NRV : inventaire vu ({} octets)", packet.len());
            lire_equipement(packet)
        }
        [0x56, 0x36] => {
            // Marqueur de diagnostic : le défilement du Combat Power arrive en
            // plusieurs paquets et on garde le plus grand. Sans voir chacun
            // d'eux, impossible de distinguer « il en manque » de « il est
            // mal lu ». À retirer une fois la chaîne validée.
            info!(
                "XIII NRV : Combat Power vu ({} octets, valeur {:?})",
                packet.len(),
                u32_le(packet, 3)
            );
            lire_combat_power(packet)
        }
        [0x00, 0x90] => {
            info!("XIII NRV : pets vus ({} octets)", packet.len());
            lire_pets(packet)
        }
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
        // L'Item Level tient sur deux octets : lus sur quatre, la version
        // imbriquée ramenait une valeur aberrante et la fiche était rejetée.
        let (Some(serveur), Some(niveau), Some(item_level)) = (
            u16_le(packet, fin),
            u32_le(packet, fin + 7),
            u16_le(packet, fin + 11),
        ) else {
            continue;
        };
        if !(1..=200).contains(&niveau)
            || !(100..=60_000).contains(&item_level)
            || !(1..=9999).contains(&serveur)
        {
            continue;
        }
        let item_level = item_level as u32;
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
    // Changement de personnage : ce qui a été lu il y a longtemps appartient au
    // précédent, et une fiche mélangée serait pire que pas de fiche du tout. Ce
    // qui vient d'arriver, lui, fait partie de la même entrée en jeu que ce nom.
    if e.nom.as_deref().is_some_and(|precedent| precedent != nom) {
        info!(
            "XIII NRV : changement de personnage ({} → {}), on repart de cette entrée en jeu",
            e.nom.as_deref().unwrap_or(""),
            nom
        );
        e.changer_de_personnage(std::time::Instant::now());
    }
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
        info!("XIII NRV : inventaire lu mais aucune pièce reconnue");
        return;
    }
    info!("XIII NRV : {} pièces d'équipement lues", pieces.len());
    pieces.sort_by_key(|p| p.emplacement);
    let mut e = etat().lock();
    e.equipement = pieces;
    e.equipement_vu_le = Some(std::time::Instant::now());
}

// ---------------------------------------------------------------------------
// 56 36 — Combat Power
// ---------------------------------------------------------------------------

/// Trois petits paquets de 19 octets font défiler le compteur à l'entrée en jeu
/// (77 148 → 111 394 → 132 462). Garder « la dernière valeur reçue » donnait un
/// Combat Power faux quand on n'attrapait qu'une étape de ce défilement.
///
/// Règle retenue : deux valeurs séparées de moins de cinq secondes font partie
/// du même défilement, on garde la plus grande. Au-delà, c'est un vrai
/// changement (équipement, niveau) et la nouvelle valeur remplace l'ancienne.
fn lire_combat_power(packet: &[u8]) {
    if packet.len() != 19 {
        return;
    }
    // Les neuf octets nuls de la mise en forme. Un vrai paquet les porte tous :
    //   16 56 36 <valeur u32> 00*4 <second champ u32> 00*4
    // Cela ne suffit pas à écarter un paquet fabriqué par une désynchronisation
    // — celui observé le 30/09 les avait tous — mais écarte les coïncidences.
    if packet[7..11].iter().any(|&o| o != 0) || packet[15..19].iter().any(|&o| o != 0) {
        return;
    }
    let Some(valeur) = u32_le(packet, 3) else {
        return;
    };
    if !(1_000..100_000_000).contains(&valeur) {
        return;
    }

    let maintenant = std::time::Instant::now();
    let mut e = etat().lock();
    let meme_defilement = e
        .cp_vu_le
        .is_some_and(|t| maintenant.duration_since(t).as_secs() < 5);

    if meme_defilement {
        e.cp_paquets = e.cp_paquets.saturating_add(1);
        e.cp_en_attente = Some(e.cp_en_attente.unwrap_or(0).max(valeur));
    } else {
        e.cp_paquets = 1;
        e.cp_en_attente = Some(valeur);
    }
    e.cp_vu_le = Some(maintenant);

    // Un paquet isolé ne s'impose pas.
    //
    // À l'entrée en jeu, le compteur défile : trois paquets en moins d'une
    // seconde (77 148 → 111 394 → 132 462). Sur la session de dix heures du
    // 30/09/2026, un unique paquet `56 36` est apparu, isolé, parfaitement
    // conforme — neuf octets nuls compris — et annonçait 3 732 pour un
    // personnage qui en affiche 132 000. Le découpage s'était perdu : sur dix
    // heures le garde-fou s'est déclenché plus de deux cents fois, des paquets
    // ayant été perdus à la capture. Accepter ce chiffre l'aurait publié sur le
    // site à la place du vrai.
    //
    // On attend donc une corroboration : deux paquets au moins dans la même
    // fenêtre. Mieux vaut un Combat Power un peu ancien qu'un Combat Power faux.
    if e.cp_paquets >= 2 {
        if let Some(retenue) = e.cp_en_attente {
            e.combat_power = Some(retenue);
        }
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
        let mut e = etat().lock();
        e.pets = familles;
        e.pets_vu_le = Some(std::time::Instant::now());
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
