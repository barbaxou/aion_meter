//! Vérifie la lecture des quatre paquets contre un enregistrement réel.
//!
//! Le fichier d'enregistrement vit hors du dépôt (il contient le trafic d'un
//! compte). Si le chemin n'existe pas — sur une autre machine, ou pour qui
//! récupère ce dépôt — le test se contente de le signaler et passe.
//!
//!     cargo test --test xiiinrv_lecture -- --nocapture
//!
//! Ce qui est attendu vient du jeu lui-même, relevé à l'écran par barbaxou :
//! Barbaxx, niveau 45, Item Level 3009, Combat Power 132 462, PV 25 835,
//! PM 5 678, 27 pièces d'équipement, 5 familles de pets et 35 effets.

use std::sync::Arc;

use xiiinrv_meter_lib::capture::framing::{self, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};
use xiiinrv_meter_lib::xiiinrv::collecte;

/// Parcourt un flux comme `StreamProcessor::consume_stream` le fait : paquets
/// bruts, lots compressés, et lots imbriqués dans un lot.
///
/// Nous avions notre propre découpeur, dans `xiiinrv/tampon.rs`. Il rejouait
/// correctement un journal entier mais n'a jamais lu un seul paquet en direct :
/// démarrant au milieu du flux, il ne se recalait pas. Il est supprimé, et ces
/// tests passent désormais par `capture::framing`, c'est-à-dire par le découpage
/// que le meter emploie réellement. Voir `docs/DECISION-REPARTIR-DE-A2TOOLS.md`
/// du dépôt du site.
fn parcourir(flux: &[u8], voir: &mut impl FnMut(&[u8])) {
    for cadre in &framing::walk(flux).frames {
        match cadre.kind {
            FrameKind::Bundle => deballer(cadre.payload(flux), voir),
            FrameKind::Packet => voir(cadre.bytes(flux)),
        }
    }
}

/// Décompresse un lot et parcourt ce qu'il contient, imbrication comprise.
fn deballer(charge: &[u8], voir: &mut impl FnMut(&[u8])) {
    let Some(contenu) = framing::decompress_bundle(charge) else {
        return;
    };
    for cadre in &framing::walk_inner(&contenu).frames {
        match cadre.kind {
            FrameKind::Bundle => deballer(cadre.payload(&contenu), voir),
            FrameKind::Packet => voir(cadre.bytes(&contenu)),
        }
    }
}

/// Un lecteur monté comme en production. Notre `observer()` est appelé depuis
/// `parse_perfect_packet` : il n'y a rien à brancher ici, c'est le point même
/// de l'accroche.
fn lecteur() -> StreamProcessor {
    StreamProcessor::new(
        Arc::new(DataStorage::new()),
        Arc::new(SkillLookup::new()),
        Arc::new(NpcLookup::new()),
    )
}

/// Donne un flux au lecteur morceau par morceau, exactement comme le
/// dispatcher : accumuler, consommer ce qui est complet, garder le reste.
fn alimenter(lecteur: &mut StreamProcessor, morceaux: &[Vec<u8>]) {
    let mut tampon = PacketAccumulator::new();
    for morceau in morceaux {
        tampon.append(morceau);
        let consommes = lecteur.consume_stream(tampon.snapshot());
        if consommes > 0 {
            tampon.discard_bytes(consommes);
        }
    }
}

/// Les deux tests partagent le même interrupteur de lecture et le même état :
/// ils ne peuvent pas tourner en même temps, sinon l'un ferme ce que l'autre
/// vient d'ouvrir. Ce verrou les fait passer l'un après l'autre.
static VERROU: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// L'opcode d'un paquet, juste pour le décompte affiché : on saute la longueur
/// (varint, un octet dans l'immense majorité des cas) et on lit deux octets.
fn opcode(p: &[u8]) -> Option<(u8, u8)> {
    let lus = if p.first().copied().unwrap_or(0) & 0x80 == 0 { 1 } else { 2 };
    Some((*p.get(lus)?, *p.get(lus + 1)?))
}

const JOURNAL: &str =
    r"D:\9 - meters aion\kuroukihime\Aion2DpsMeter-v1.10.3.302-win-x64\PacketLogs\packets_20260920_090846.txt";

#[test]
fn lit_la_fiche_complete_depuis_un_enregistrement_reel() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(contenu) = std::fs::read_to_string(JOURNAL) else {
        eprintln!("enregistrement absent, test ignoré : {}", JOURNAL);
        return;
    };

    // Le partage doit être ouvert, sinon rien n'est lu — c'est justement la garantie.
    collecte::vider();
    collecte::ouvrir_lecture(true);

    // Un paquet de plusieurs kilo-octets est découpé sur plusieurs lignes du
    // journal : on reconstitue d'abord chaque flux bout à bout, exactement comme
    // le meter le fait en direct, puis on le parcourt.
    let mut flux: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for ligne in contenu.lines() {
        let champs: Vec<&str> = ligne.trim_end().split('|').collect();
        if champs.len() != 3 || champs[1] == "STREAMKEY" {
            continue;
        }
        let Some(octets) = hex_vers_octets(champs[2].trim()) else {
            continue;
        };
        flux.entry(champs[1].to_string()).or_default().extend(octets);
    }

    let mut paquets = 0usize;
    let mut opcodes: std::collections::HashMap<(u8, u8), usize> = std::collections::HashMap::new();
    for (_cle, tampon) in &flux {
        parcourir(tampon, &mut |p| {
            paquets += 1;
            if let Some(o) = opcode(p) {
                *opcodes.entry(o).or_insert(0usize) += 1;
            }
            collecte::observer(p);
        });
    }

    let e = collecte::lire_etat();
    println!("{} paquets parcourus", paquets);
    let mut liste: Vec<_> = opcodes.iter().collect();
    liste.sort_by(|a, b| b.1.cmp(a.1));
    println!("opcodes les plus fréquents :");
    for ((a, b), n) in liste.iter().take(12) {
        println!("   {:02x} {:02x} -> {}", a, b, n);
    }
    for cible in [(0x33u8, 0x36u8), (0x11, 0x56), (0x56, 0x36), (0x00, 0x90)] {
        println!("   cible {:02x} {:02x} : {}", cible.0, cible.1,
                 opcodes.get(&cible).copied().unwrap_or(0));
    }
    println!(
        "nom={:?} niveau={:?} itemLevel={:?} cp={:?} pv={:?} pm={:?} pièces={} pets={}",
        e.nom,
        e.niveau,
        e.item_level,
        e.combat_power,
        e.pv,
        e.pm,
        e.equipement.len(),
        e.pets.len()
    );

    assert_eq!(e.nom.as_deref(), Some("Barbaxx"), "nom du personnage");
    assert_eq!(e.serveur.as_deref(), Some("Kasaka (TW)"), "serveur");
    assert_eq!(e.niveau, Some(45), "niveau");
    assert_eq!(e.item_level, Some(3009), "Item Level");
    assert_eq!(e.combat_power, Some(132462), "Combat Power");
    assert_eq!(e.pv, Some(25835), "PV");
    assert_eq!(e.pm, Some(5678), "PM");
    assert_eq!(e.equipement.len(), 27, "pièces d'équipement");

    // L'arme, relevée à l'écran : Nathara Bow +10, emplacement 1.
    let arme = e
        .equipement
        .iter()
        .find(|p| p.emplacement == 1)
        .expect("arme absente");
    assert_eq!(arme.item_id, 110430111);
    assert_eq!(arme.enchantement, 10);

    assert_eq!(e.pets.len(), 5, "familles de pets");
    let effets: usize = e.pets.iter().map(|g| g.effets.len()).sum();
    assert_eq!(effets, 35, "effets de pets");

    let fera = e.pets.iter().find(|g| g.genus == "Fera").expect("Fera absent");
    assert_eq!(fera.niveau, 8);
    assert_eq!(fera.xp, 1410);
    // Effet de base : Blocage 35, qualité Unique (4).
    let base = fera.effets.iter().find(|f| f.slot == 0).expect("effet de base");
    assert_eq!((base.rarete, base.stat, base.valeur), (4, 255, 35));
}

/// Session du 27/09/2026 : le joueur entre en jeu **sans ouvrir l'écran des
/// pets**. La fiche de personnage y est imbriquée dans un paquet `60 88` au lieu
/// d'arriver seule — c'est ce cas qui échappait à la lecture.
#[test]
fn lit_la_fiche_imbriquee_sans_ouvrir_les_pets() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    let journal = std::path::Path::new(&std::env::var("APPDATA").unwrap_or_default())
        .join("gg.xiiinrv.meter")
        .join("packets_20260927_111258.txt");
    let Ok(contenu) = std::fs::read_to_string(&journal) else {
        eprintln!("enregistrement absent, test ignoré : {}", journal.display());
        return;
    };

    collecte::vider();
    collecte::ouvrir_lecture(true);

    let mut flux: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for ligne in contenu.lines() {
        let champs: Vec<&str> = ligne.trim_end().split('|').collect();
        if champs.len() != 3 || champs[1] == "STREAMKEY" {
            continue;
        }
        if let Some(octets) = hex_vers_octets(champs[2].trim()) {
            flux.entry(champs[1].to_string()).or_default().extend(octets);
        }
    }
    for tampon in flux.values() {
        parcourir(tampon, &mut |p| collecte::observer(p));
    }

    let e = collecte::lire_etat();
    println!(
        "fiche imbriquée : nom={:?} niveau={:?} itemLevel={:?} cp={:?} pièces={}",
        e.nom,
        e.niveau,
        e.item_level,
        e.combat_power,
        e.equipement.len()
    );
    assert_eq!(e.nom.as_deref(), Some("Barbaxx"), "nom");
    assert_eq!(e.niveau, Some(45), "niveau");
    assert_eq!(e.item_level, Some(3009), "Item Level");
    // Le compteur défile à l'entrée en jeu : on doit garder la valeur finale,
    // pas une étape du défilement.
    assert_eq!(e.combat_power, Some(132462), "Combat Power");
    assert!(e.equipement.len() >= 20, "équipement lu");
}

#[test]
fn ne_lit_rien_sans_jeton() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(contenu) = std::fs::read_to_string(JOURNAL) else {
        eprintln!("enregistrement absent, test ignoré");
        return;
    };
    collecte::vider();
    collecte::ouvrir_lecture(false); // interrupteur fermé : cas par défaut

    let mut tampon: Vec<u8> = Vec::new();
    for ligne in contenu.lines().take(5000) {
        let champs: Vec<&str> = ligne.trim_end().split('|').collect();
        if champs.len() == 3 {
            if let Some(octets) = hex_vers_octets(champs[2].trim()) {
                tampon.extend(octets);
            }
        }
    }
    parcourir(&tampon, &mut |p| collecte::observer(p));

    let e = collecte::lire_etat();
    assert!(e.nom.is_none(), "un nom a été lu alors que le partage est fermé");
    assert!(e.equipement.is_empty(), "de l'équipement a été lu");
    assert!(e.pets.is_empty(), "des pets ont été lus");
}

/// Une coupe au milieu d'un paquet ne doit rien faire perdre au reste du flux.
///
/// Le tampon d'avant le verrouillage commence là où le meter a commencé à
/// écouter, donc presque toujours au milieu d'un paquet. Le découpeur se
/// resynchronise octet par octet, mais un mauvais départ peut lui faire avaler
/// la longueur d'un paquet de travers et sauter les suivants.
///
/// C'est arrivé en vrai le 29/09/2026. Le Combat Power défile en trois paquets
/// (77 148 → 111 394 → 132 462) et on garde le plus grand ; le site a reçu
/// 77 148. La première marche avait été lue, les deux autres perdues dans la
/// désynchronisation. D'où ce test : on coupe le flux à quatre cents endroits, tous
/// bien avant le défilement, et on exige que la valeur finale soit toujours la
/// bonne.
#[test]
fn une_coupe_au_milieu_dun_paquet_ne_fait_rien_perdre() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(contenu) = std::fs::read_to_string(JOURNAL) else {
        eprintln!("enregistrement absent, test ignoré : {}", JOURNAL);
        return;
    };

    // Le plus gros flux du journal, reconstitué bout à bout.
    let mut flux: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for ligne in contenu.lines() {
        let champs: Vec<&str> = ligne.trim_end().split('|').collect();
        if champs.len() != 3 || champs[1] == "STREAMKEY" {
            continue;
        }
        if let Some(octets) = hex_vers_octets(champs[2].trim()) {
            flux.entry(champs[1].to_string()).or_default().extend(octets);
        }
    }
    let Some(entier) = flux.into_values().max_by_key(|o| o.len()) else {
        panic!("aucun flux dans le journal");
    };

    // Le flux entier, qui contient le défilement complet : c'est la référence.
    // On le parcourt pour relever la valeur que le jeu a réellement envoyée en
    // dernier — c'est elle, et pas une autre, que chaque coupe devra rendre.
    let morceau = &entier[..];
    let mut vues: Vec<u32> = Vec::new();
    parcourir(morceau, &mut |p| {
        if p.len() == 19 && p.get(1) == Some(&0x56) && p.get(2) == Some(&0x36) {
            if let Some(o) = p.get(3..7) {
                vues.push(u32::from_le_bytes([o[0], o[1], o[2], o[3]]));
            }
        }
    });
    println!("défilement présent dans le flux : {:?}", vues);
    let attendu = *vues.iter().max().expect("aucun Combat Power dans le flux");

    // Cent coupes, toutes au milieu d'un paquet. Le défilement est bien plus
    // loin : aucune ne le retire, donc aucune n'a d'excuse pour le manquer.
    //
    // Chaque coupe rejoue le flux tronqué par le cycle du dispatcher — accumuler,
    // consommer, garder le reste — avec un lecteur neuf, donc un flux vierge.
    const COUPES: usize = 100;
    let mut faux = Vec::new();
    let mut perdus = Vec::new();
    for coupe in 1..=COUPES {
        collecte::vider();
        collecte::ouvrir_lecture(true);
        let mut lu = lecteur();
        alimenter(&mut lu, &[morceau[coupe..].to_vec()]);
        match collecte::lire_etat().combat_power {
            Some(lu) if lu != attendu => faux.push((coupe, lu)),
            None => perdus.push(coupe),
            _ => {}
        }
    }
    collecte::ouvrir_lecture(false);
    collecte::vider();

    // Le seul défaut inacceptable : remonter au site un chiffre qui n'est pas
    // celui du jeu. Le recalage préfère ne rien relire que relire de travers,
    // donc ceci doit être vide sans discussion.
    assert!(
        faux.is_empty(),
        "Combat Power attendu {} ; coupes qui en remontent un autre (coupe, lu) : {:?}",
        attendu,
        faux
    );

    // Ne rien relire du tout reste possible, et c'est assumé : partant au milieu
    // d'un paquet, la resynchronisation d'A2Tools avance d'un octet à la fois et
    // ne retrouve pas toujours l'alignement avant que les paquets visés passent.
    // Mesuré ici : un quart des coupes perdent l'entrée en jeu, et le chiffre ne
    // bouge pas qu'on alimente le lecteur d'un bloc ou par morceaux de 1500
    // octets — c'est le point de départ qui décide, pas le découpage en morceaux.
    //
    // Pourquoi cela reste acceptable : en direct, le flux n'est pas pris en
    // cours de route. Le verrouillage du port tombe après 132 octets sur un
    // flux de jeu réel (le jeu émet le terminateur de enregistrement une
    // vingtaine de fois par seconde, même au repos), et le lecteur suit donc le
    // flux depuis son début. C'est ce que vérifie
    // `rejoue_une_entree_en_jeu_reelle_sans_rien_perdre`, sur le vrai flux et
    // avec le vrai cycle. Le cas couvert ici — démarrer au milieu — n'arrive
    // qu'au lancement du meter en cours de session, et se rattrape à l'entrée en
    // jeu suivante.
    //
    // Le plafond n'est donc pas un objectif de qualité, c'est un détecteur de
    // régression : si cette proportion grimpe, la resynchronisation s'est
    // dégradée et il faut regarder pourquoi.
    println!("{} coupes perdent l'entrée en jeu : {:?}", perdus.len(), perdus);
    assert!(
        perdus.len() * 100 <= COUPES * 35,
        "{} coupes sur {} ne relisent rien ({} %) : le recalage s'est dégradé",
        perdus.len(),
        COUPES,
        perdus.len() * 100 / COUPES
    );
}

/// Rejoue une vraie entrée en jeu, morceau par morceau, comme le meter la reçoit.
///
/// Le 29/09/2026 à 12:12, barbaxou est entré en jeu avec Barbaxx sur une
/// connexion toute neuve. Le journal des paquets montre que le jeu a envoyé les
/// trois paquets du défilement du Combat Power (77 148 → 111 394 → 132 462) et
/// l'inventaire complet, 13 732 octets. Le meter n'a vu que le premier Combat
/// Power, et aucune pièce d'équipement : il lisait alors les paquets là où
/// A2Tools les avait découpés, et ce découpage-là en perdait.
///
/// Ce test rejoue exactement ces octets, morceau par morceau et dans l'ordre,
/// par le découpage de la version sur laquelle nous sommes revenus. Il répond
/// donc directement à la question qui nous avait fait partir : ce découpage-ci
/// perd-il encore l'inventaire et le défilement du Combat Power ?
#[test]
fn rejoue_une_entree_en_jeu_reelle_sans_rien_perdre() {
    const JOURNAL_1212: &str = r"D:\9 - meters aion\xiiinrv\entree_en_jeu_20260929_1212.txt";
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(contenu) = std::fs::read_to_string(JOURNAL_1212) else {
        eprintln!("enregistrement absent, test ignoré : {}", JOURNAL_1212);
        return;
    };

    collecte::vider();
    collecte::ouvrir_lecture(true);

    // Le journal couvre deux connexions successives : celle d'avant le
    // redémarrage du jeu, et la nouvelle. Leur clé de flux est la même — le port
    // du serveur — donc les coller bout à bout désynchroniserait tout. On ne
    // garde que la seconde.
    const DEPART: &str = "2026-09-29T12:12:16";
    let mut octets_par_morceau: Vec<Vec<u8>> = Vec::new();
    for ligne in contenu.lines() {
        let champs: Vec<&str> = ligne.trim_end().split('|').collect();
        if champs.len() != 3 || champs[1] != "Client:61944" || champs[0] < DEPART {
            continue;
        }
        if let Some(octets) = hex_vers_octets(champs[2].trim()) {
            octets_par_morceau.push(octets);
        }
    }
    let morceaux = octets_par_morceau.len();
    let mut lu = lecteur();
    alimenter(&mut lu, &octets_par_morceau);

    let e = collecte::lire_etat();
    println!(
        "{} morceaux rejoués : cp={:?} pièces={} niveau={:?}",
        morceaux,
        e.combat_power,
        e.equipement.len(),
        e.niveau
    );
    collecte::ouvrir_lecture(false);
    collecte::vider();

    assert!(morceaux > 100, "journal trop court : {} morceaux", morceaux);
    assert_eq!(
        e.combat_power,
        Some(132462),
        "le défilement du Combat Power doit être lu en entier, pas seulement sa première marche"
    );
    assert!(
        !e.equipement.is_empty(),
        "l'inventaire de 13 732 octets était bien dans le flux : aucune pièce lue"
    );
}

/// Changer de personnage ne doit pas produire une fiche mélangée.
///
/// À l'entrée en jeu, le nom arrive **en dernier** : le 29/09/2026, huit
/// secondes après l'équipement (13:31:22 contre 13:31:30). Entre les deux, la
/// fiche en mémoire porte l'équipement du nouveau personnage et le nom de
/// l'ancien. Un envoi tombant dans cette fenêtre attribuerait le stuff d'un
/// personnage à un autre. Les PV et PM sont pires encore : `lire_fiche` ne les
/// écrase que si la fiche en contient, et toutes n'en contiennent pas — ceux du
/// personnage précédent restaient donc indéfiniment.
#[test]
fn changer_de_personnage_ne_melange_pas_deux_fiches() {
    use std::time::{Duration, Instant};

    let maintenant = Instant::now();
    let garni = |age: Duration| {
        let lu_le = maintenant.checked_sub(age);
        collecte::Etat {
            nom: Some("Barbaxx".to_string()),
            pv: Some(25_835),
            pm: Some(5_678),
            combat_power: Some(132_462),
            cp_vu_le: lu_le,
            equipement: vec![collecte::Piece {
                emplacement: 1,
                item_id: 110_430_111,
                enchantement: 12,
                conteneur: 0x0B,
            }],
            equipement_vu_le: lu_le,
            ..Default::default()
        }
    };

    // Neuf secondes : c'est l'écart mesuré entre l'équipement et le nom. Tout
    // cela fait partie de la même entrée en jeu et doit être gardé.
    let mut e = garni(Duration::from_secs(9));
    e.changer_de_personnage(maintenant);
    assert_eq!(e.equipement.len(), 1, "équipement de la même entrée en jeu, à garder");
    assert_eq!(e.combat_power, Some(132_462), "Combat Power de la même entrée en jeu");

    // Deux minutes : cela appartient au personnage précédent.
    let mut e = garni(Duration::from_secs(120));
    e.changer_de_personnage(maintenant);
    assert!(e.equipement.is_empty(), "équipement d'un autre personnage, à jeter");
    assert_eq!(e.combat_power, None, "Combat Power d'un autre personnage");

    // Les PV et PM partent dans tous les cas : ils viennent de la fiche, et la
    // fiche qui suit ce changement les remplira si elle les porte.
    assert_eq!(e.pv, None, "les PV d'un autre personnage ne doivent pas rester");
    assert_eq!(e.pm, None, "les PM d'un autre personnage ne doivent pas rester");
    let mut recent = garni(Duration::from_secs(9));
    recent.changer_de_personnage(maintenant);
    assert_eq!(recent.pv, None, "les PV partent même pour une bascule immédiate");

    // Et le tout sur l'enregistrement réel : même paquet de fiche, autre nom.
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(contenu) = std::fs::read_to_string(JOURNAL) else {
        eprintln!("enregistrement absent, reste du test ignoré : {}", JOURNAL);
        return;
    };

    collecte::vider();
    collecte::ouvrir_lecture(true);
    let mut flux: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for ligne in contenu.lines() {
        let champs: Vec<&str> = ligne.trim_end().split('|').collect();
        if champs.len() != 3 || champs[1] == "STREAMKEY" {
            continue;
        }
        if let Some(octets) = hex_vers_octets(champs[2].trim()) {
            flux.entry(champs[1].to_string()).or_default().extend(octets);
        }
    }
    let mut fiche: Option<Vec<u8>> = None;
    for tampon in flux.values() {
        parcourir(tampon, &mut |p| {
            if opcode(p) == Some((0x33, 0x36)) {
                fiche = Some(p.to_vec());
            }
            collecte::observer(p);
        });
    }
    let avant = collecte::lire_etat();
    assert_eq!(avant.nom.as_deref(), Some("Barbaxx"));
    assert_eq!(avant.equipement.len(), 27);

    let mut autre = fiche.expect("aucun paquet de fiche dans l'enregistrement");
    let position = autre
        .windows(7)
        .position(|f| f == b"Barbaxx")
        .expect("nom introuvable dans le paquet de fiche");
    autre[position..position + 7].copy_from_slice(b"Barbaxy");
    collecte::observer(&autre);

    let apres = collecte::lire_etat();
    collecte::ouvrir_lecture(false);
    collecte::vider();
    assert_eq!(apres.nom.as_deref(), Some("Barbaxy"), "le nouveau nom remplace l'ancien");
    assert_eq!(
        apres.equipement.len(),
        27,
        "l'équipement venait d'être lu : même entrée en jeu, on le garde"
    );
}

/// Un Combat Power isolé ne doit pas s'imposer.
///
/// À l'entrée en jeu le compteur défile : trois paquets en moins d'une seconde.
/// Sur la session de dix heures du 30/09/2026, un unique paquet `56 36` est
/// apparu, isolé et parfaitement conforme — les neuf octets nuls de la mise en
/// forme y étaient — annonçant 3 732 pour un personnage qui en affiche 132 000.
/// Le découpage s'était perdu : le garde-fou s'était déclenché plus de deux cents
/// fois sur la session, des paquets ayant été perdus à la capture. Un contrôle de
/// structure ne l'aurait pas écarté ; seule la corroboration le fait.
#[test]
fn un_combat_power_isole_ne_simpose_pas() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());

    // Les octets réels, relevés dans les deux enregistrements.
    let defilement: [&[u8]; 3] = [
        &[0x16, 0x56, 0x36, 0x5c, 0x2d, 0x01, 0, 0, 0, 0, 0, 0xac, 0x06, 0x02, 0, 0, 0, 0, 0],
        &[0x16, 0x56, 0x36, 0x22, 0xb3, 0x01, 0, 0, 0, 0, 0, 0xac, 0x06, 0x02, 0, 0, 0, 0, 0],
        &[0x16, 0x56, 0x36, 0x6e, 0x05, 0x02, 0, 0, 0, 0, 0, 0xac, 0x06, 0x02, 0, 0, 0, 0, 0],
    ];
    let isole: &[u8] = &[
        0x16, 0x56, 0x36, 0x94, 0x0e, 0, 0, 0, 0, 0, 0, 0x94, 0x0e, 0, 0, 0, 0, 0, 0,
    ];

    // Le défilement complet : la plus grande valeur est retenue.
    collecte::vider();
    collecte::ouvrir_lecture(true);
    for p in defilement {
        collecte::observer(p);
    }
    assert_eq!(
        collecte::lire_etat().combat_power,
        Some(132_462),
        "le défilement de trois paquets doit donner sa plus grande valeur"
    );

    // Le paquet isolé, seul : rien ne doit être retenu.
    collecte::vider();
    collecte::observer(isole);
    assert_eq!(
        collecte::lire_etat().combat_power,
        None,
        "un paquet isolé ne suffit pas, même parfaitement conforme"
    );

    // Et il ne doit pas écraser un Combat Power déjà établi.
    collecte::vider();
    for p in defilement {
        collecte::observer(p);
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    collecte::observer(isole);
    assert_eq!(
        collecte::lire_etat().combat_power,
        Some(132_462),
        "un paquet isolé ne doit pas remplacer un Combat Power corroboré"
    );

    collecte::ouvrir_lecture(false);
    collecte::vider();
}

/// Un message du jeu ne doit pas devenir le nom de la fiche.
///
/// Le 02/10/2026, le champ « Nom#ID » d'A2Tools contenait « demande votre
/// autorisation » : un message du jeu, détecté comme nom de personnage et
/// enregistré dans les réglages, où il persistait d'une version à l'autre. Notre
/// fiche s'en sert en secours tant que la fiche de personnage n'a pas été lue —
/// elle serait donc partie vers le site sous ce nom, créant un personnage que le
/// Roster n'aurait jamais pu rattacher à personne.
#[test]
fn un_message_du_jeu_ne_devient_pas_un_nom_de_personnage() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());
    collecte::vider();
    collecte::ouvrir_lecture(true);

    for refuse in [
        "demande votre autorisation",
        "Ni****l",             // nom masqué par le jeu
        "a",                   // trop court
        "Barbaxx le magnifique et plus encore",
        " ",
    ] {
        collecte::nom_detecte(Some(refuse.to_string()));
        assert_eq!(
            collecte::lire_etat().nom,
            None,
            "« {} » ne doit pas être retenu comme nom",
            refuse
        );
    }

    for accepte in ["Barbaxx", "Eztheim", "Ar", "Joueur2026"] {
        collecte::nom_detecte(Some(accepte.to_string()));
        assert_eq!(
            collecte::lire_etat().nom.as_deref(),
            Some(accepte),
            "« {} » est un nom de personnage valable",
            accepte
        );
    }

    collecte::nom_detecte(None);
    collecte::ouvrir_lecture(false);
    collecte::vider();
}

fn hex_vers_octets(hexa: &str) -> Option<Vec<u8>> {
    // Le journal commence par une marque d'ordre des octets : on écarte toute
    // ligne qui n'est pas strictement de l'hexadécimal.
    if hexa.is_empty() || hexa.len() % 2 != 0 || !hexa.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..hexa.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hexa[i..i + 2], 16).ok())
        .collect()
}

/// Appliquer les réglages ouvre la lecture, et les retirer la referme.
///
/// Ce test existe à cause d'un défaut précis. `configurer()` n'était appelé que
/// depuis `update_settings`, donc **uniquement quand on touchait un réglage dans
/// l'interface**. Au lancement du meter, personne ne relisait `settings.json` :
/// le partage restait inactif en mémoire, la lecture des paquets était fermée,
/// et pourtant la case de l'interface s'affichait cochée puisqu'elle lit le
/// fichier. Plus rien n'était lu tant que le membre n'allait pas décocher puis
/// recocher sa case.
///
/// Vu de l'extérieur, cela ressemblait à des pertes de reconnaissance
/// aléatoires : certaines sessions remontaient la fiche, d'autres non, sans
/// raison apparente. barbaxou l'a signalé le 05/10/2026, capture à l'appui — son
/// écran annonçait « Partage décoché : rien n'est lu » avec l'interrupteur
/// allumé.
///
/// La vraie protection est la signature de `demarrer`, qui exige désormais les
/// réglages et rend l'oubli impossible. Ce test garde l'invariant qu'elle sert.
#[test]
fn appliquer_les_reglages_ouvre_la_lecture() {
    use xiiinrv_meter_lib::xiiinrv::envoi::configurer;

    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());

    configurer(Some("jeton-de-test".to_string()), None, true);
    assert!(
        collecte::lecture_ouverte(),
        "partage actif : la lecture doit être ouverte, sinon aucun paquet n'est regardé"
    );

    configurer(Some("jeton-de-test".to_string()), None, false);
    assert!(
        !collecte::lecture_ouverte(),
        "partage inactif : la lecture doit être fermée, rien ne doit être lu"
    );

    // Et l'état rendu à l'interface doit dire la même chose que la lecture :
    // c'est leur désaccord qui avait mis la puce à l'oreille.
    configurer(Some("jeton-de-test".to_string()), None, true);
    let etat = xiiinrv_meter_lib::xiiinrv::etat_partage();
    assert!(
        etat.actif && collecte::lecture_ouverte(),
        "l'état affiché et la lecture réelle doivent s'accorder"
    );

    configurer(None, None, false);
    collecte::vider();
}

/// Changer de personnage doit faire relire la fiche imbriquée.
///
/// La fiche arrive imbriquée dans le gros paquet d'entrée en jeu, et
/// `observer()` ne va la chercher là que derrière une condition :
/// `etat().niveau.is_none()`. Une fois un niveau connu, cette porte se refermait
/// définitivement. Au changement de personnage, la nouvelle fiche n'était donc
/// plus jamais lue : `lire_fiche` n'étant pas atteint, le
/// `changer_de_personnage()` qu'il contient ne tirait pas non plus, et le nom,
/// le niveau et l'Item Level du personnage précédent restaient en mémoire.
///
/// Vu du site, la fiche restait collée au premier personnage joué. barbaxou l'a
/// signalé le 05/10/2026 : après bascule, le site affichait toujours « Barbax »
/// alors que le meter avait bien reconnu « Barbaxou ».
///
/// Les octets viennent de sa capture `packets_20261005_231354.txt` : les deux
/// personnages y apparaissent à 23:19:44 et 23:22:09, avec la même disposition
/// de champs. Ils sont recopiés ici tels quels pour que le test porte sur du
/// réel et non sur une disposition supposée.
#[test]
fn changer_de_personnage_fait_relire_la_fiche_imbriquee() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());

    /// Un paquet d'entrée en jeu : un opcode extérieur qui n'est **pas**
    /// `33 36`, la marque `33 36` à l'intérieur, puis la fiche.
    ///
    /// `suite` est la séquence relevée après le nom dans la capture :
    /// serveur `ff 08`, classe u32, un octet, niveau u32, Item Level u16.
    fn paquet_imbrique(nom: &str, suite: &[u8]) -> Vec<u8> {
        let mut p = vec![0x60, 0x60, 0x88]; // longueur (varint), puis opcode 60 88
        p.extend_from_slice(&[0x33, 0x36]); // la marque, à l'intérieur du paquet
        p.extend_from_slice(&[0x00, 0x00]);
        p.extend_from_slice(nom.as_bytes());
        p.extend_from_slice(suite);
        p.extend_from_slice(&[0x00; 16]);
        p
    }

    // Relevés dans la capture, octet pour octet.
    let suite_barbax = [
        0xff, 0x08, 0x0f, 0x00, 0x00, 0x00, 0x02, 0x2d, 0x00, 0x00, 0x00, 0x57, 0x06,
    ];
    let suite_barbaxou = [
        0xff, 0x08, 0x1f, 0x00, 0x00, 0x00, 0x02, 0x2d, 0x00, 0x00, 0x00, 0x4a, 0x05,
    ];

    collecte::vider();
    collecte::ouvrir_lecture(true);

    // Premier personnage : la porte est ouverte, puisqu'aucun niveau n'est connu.
    collecte::nom_detecte(Some("Barbax".to_string()));
    collecte::observer(&paquet_imbrique("Barbax", &suite_barbax));
    let e = collecte::lire_etat();
    assert_eq!(
        e.nom.as_deref(),
        Some("Barbax"),
        "témoin : la fiche imbriquée du premier personnage doit être lue, \
         sinon le reste du test ne prouve rien"
    );
    assert_eq!(e.niveau, Some(45), "témoin : le niveau du premier personnage");
    assert_eq!(e.item_level, Some(1623), "témoin : l'Item Level du premier personnage");

    // Bascule : le jeu annonce l'autre personnage. C'est le signal dont on
    // dispose réellement — `nom_detecte` est rafraîchi toutes les 15 secondes
    // depuis l'identité que le jeu donne.
    collecte::nom_detecte(Some("Barbaxou".to_string()));
    collecte::observer(&paquet_imbrique("Barbaxou", &suite_barbaxou));

    let e = collecte::lire_etat();
    assert_eq!(
        e.nom.as_deref(),
        Some("Barbaxou"),
        "la fiche du nouveau personnage doit remplacer l'ancienne ; \
         rester sur « Barbax » est le défaut signalé le 05/10/2026"
    );
    assert_eq!(
        e.item_level,
        Some(1354),
        "l'Item Level doit suivre le personnage, pas rester celui du précédent"
    );

    collecte::ouvrir_lecture(false);
    collecte::vider();
    // `vider()` ne touche pas au nom détecté, et `lire_etat()` s'en sert comme
    // filet quand la fiche n'en porte pas : le laisser ici faisait échouer
    // `ne_lit_rien_sans_jeton`, qui vérifie qu'aucun nom n'est lu.
    collecte::nom_detecte(None);
}

/// Un paquet de Combat Power isolé, mais structurellement corroboré, compte.
///
/// `lire_combat_power` exigeait **deux** paquets dans la même fenêtre de cinq
/// secondes. Cette prudence avait sa raison : le 30/09/2026, un paquet isolé
/// parfaitement conforme — neuf octets nuls compris — annonçait 3 732 pour un
/// personnage qui en affiche 132 000, et l'accepter l'aurait publié sur le site.
///
/// Mais elle échoue quand un seul paquet passe, ce qui est arrivé le
/// 06/10/2026 : un unique `56 36` à 81 451, écarté, et un Combat Power resté
/// vide sur le site de barbaxou alors que la valeur avait bien été lue.
///
/// La mesure a montré que le paquet porte **deux** entiers de 64 bits
/// consécutifs, et non un seul :
///
/// ```text
/// 16 56 36 <champ 1 u32> 00*4 <champ 2 u32> 00*4
/// ```
///
/// Relevés réels :
///
/// ```text
/// 20/09  77148 / 132780      le compteur défile, le champ 2 ne bouge pas
/// 20/09  111394 / 132780
/// 20/09  132462 / 132780     132 462 est la valeur vraie
/// 06/10  81451 / 81451       paquet isolé, déjà stabilisé
/// ```
///
/// D'où un **second avis structurel**, instantané, comme celui de Kuroukihime :
/// les deux champs dans 10 000..2 000 000 et le premier ≤ le second. Le faux
/// paquet du 30/09 annonçait 3 732, donc **sous la borne** : il reste rejeté.
/// La corroboration temporelle est conservée pour les valeurs hors fourchette —
/// un personnage débutant sous 10 000 passe encore par elle.
#[test]
fn un_combat_power_isole_mais_corrobore_est_retenu() {
    let _garde = VERROU.lock().unwrap_or_else(|e| e.into_inner());

    /// `16 56 36 <a u32> 00*4 <b u32> 00*4`
    fn paquet(a: u32, b: u32) -> Vec<u8> {
        let mut p = vec![0x16, 0x56, 0x36];
        p.extend_from_slice(&a.to_le_bytes());
        p.extend_from_slice(&[0; 4]);
        p.extend_from_slice(&b.to_le_bytes());
        p.extend_from_slice(&[0; 4]);
        assert_eq!(p.len(), 19);
        p
    }

    // Le cas du 06/10 : un seul paquet, les deux champs d'accord.
    collecte::vider();
    collecte::ouvrir_lecture(true);
    collecte::observer(&paquet(81_451, 81_451));
    assert_eq!(
        collecte::lire_etat().combat_power,
        Some(81_451),
        "un paquet isolé dont les deux champs se corroborent doit compter ; \
         l'écarter laissait le Combat Power vide sur le site"
    );

    // Témoin : le faux paquet du 30/09 reste rejeté, isolé et sous la borne.
    collecte::vider();
    collecte::observer(&paquet(3_732, 3_732));
    assert_eq!(
        collecte::lire_etat().combat_power,
        None,
        "témoin : 3 732 est sous la borne des 10 000 — c'est le faux du 30/09, \
         il ne doit pas passer, sinon la correction est pire que le défaut"
    );

    // Témoin : le défilement du 20/09 rend toujours sa valeur finale.
    collecte::vider();
    for a in [77_148u32, 111_394, 132_462] {
        collecte::observer(&paquet(a, 132_780));
    }
    assert_eq!(
        collecte::lire_etat().combat_power,
        Some(132_462),
        "témoin : le défilement doit rendre sa dernière marche, pas la première"
    );

    collecte::ouvrir_lecture(false);
    collecte::vider();
    collecte::nom_detecte(None);
}
