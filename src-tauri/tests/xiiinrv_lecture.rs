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

use xiiinrv_meter_lib::xiiinrv::collecte;
// Le découpage du flux est celui du meter lui-même, pas une copie : c'est lui
// qui relit les paquets gardés de côté avant le verrouillage du port.
use xiiinrv_meter_lib::xiiinrv::tampon::{debut_aligne, deverrouille, parcourir, recevoir, verrouille};

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

    // Un morceau qui commence sur un vrai début de paquet, et qui contient le
    // défilement complet : c'est la référence.
    let depart = debut_aligne(&entier).expect("aucun début aligné dans le flux entier");
    let morceau = &entier[depart..];
    let mut vues: Vec<u32> = Vec::new();
    parcourir(morceau, &mut |p| {
        if p.len() == 19 && p.get(1) == Some(&0x56) && p.get(2) == Some(&0x36) {
            if let Some(o) = p.get(3..7) {
                vues.push(u32::from_le_bytes([o[0], o[1], o[2], o[3]]));
            }
        }
    });
    println!("défilement présent dans le morceau : {:?}", vues);
    let attendu = *vues.iter().max().expect("aucun Combat Power dans le morceau");

    // Quatre cents coupes, toutes au milieu d'un paquet. Le défilement est bien plus
    // loin : aucune ne le retire, donc aucune n'a d'excuse pour le manquer.
    const COUPES: usize = 400;
    let mut faux = Vec::new();
    let mut perdus = Vec::new();
    for coupe in 1..=COUPES {
        collecte::vider();
        collecte::ouvrir_lecture(true);
        // Chaque coupe repart d'un flux vierge : sans ça, la fin du tampon de la
        // coupe précédente se collerait devant celui-ci.
        deverrouille();
        recevoir(4321, 8765, &morceau[coupe..]);
        verrouille(4321, 8765);
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

    // Ne rien relire du tout reste possible : certaines coupes ne laissent pas de
    // quoi reconnaître un début de paquet, et on assume de perdre l'entrée en jeu
    // plutôt que de l'inventer. Mais cela doit rester rare : la version sans
    // recalage en perdait plus d'un quart.
    println!("{} coupes perdent l'entrée en jeu : {:?}", perdus.len(), perdus);
    assert!(
        perdus.len() * 100 <= COUPES,
        "{} coupes sur {} ne relisent rien ({} %) : le recalage ne tient plus",
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
/// Ce test rejoue exactement ces octets dans notre propre réassemblage, morceau
/// par morceau et dans l'ordre. Il doit rendre les quatre paquets.
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
    let mut morceaux = 0usize;
    for ligne in contenu.lines() {
        let champs: Vec<&str> = ligne.trim_end().split('|').collect();
        if champs.len() != 3 || champs[1] != "Client:61944" || champs[0] < DEPART {
            continue;
        }
        let Some(octets) = hex_vers_octets(champs[2].trim()) else {
            continue;
        };
        if morceaux == 0 {
            // Le premier morceau sert de tampon d'avant verrouillage, puis on
            // déclare le flux du jeu : c'est la séquence réelle du dispatcher.
            recevoir(61944, 50349, &octets);
            verrouille(61944, 50349);
        } else {
            recevoir(61944, 50349, &octets);
        }
        morceaux += 1;
    }

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
