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

/// Les deux tests partagent le même interrupteur de lecture et le même état :
/// ils ne peuvent pas tourner en même temps, sinon l'un ferme ce que l'autre
/// vient d'ouvrir. Ce verrou les fait passer l'un après l'autre.
static VERROU: std::sync::Mutex<()> = std::sync::Mutex::new(());

const JOURNAL: &str =
    r"D:\9 - meters aion\kuroukihime\Aion2DpsMeter-v1.10.3.302-win-x64\PacketLogs\packets_20260920_090846.txt";

/// Entier à longueur variable : (valeur, octets lus).
fn varint(d: &[u8], o: usize) -> Option<(usize, usize)> {
    let mut valeur = 0usize;
    let mut decalage = 0;
    let mut lus = 0;
    loop {
        let octet = *d.get(o + lus)?;
        lus += 1;
        valeur |= ((octet & 0x7F) as usize) << decalage;
        if octet & 0x80 == 0 {
            return Some((valeur, lus));
        }
        decalage += 7;
        if decalage >= 32 {
            return None;
        }
    }
}

/// Même découpage que `consume_stream` d'A2Tools : longueur varint (moins 3) et
/// groupes compressés `FF FF`. Deux détails qui comptent : au premier niveau un
/// groupe occupe **un octet de plus**, et on se resynchronise octet par octet
/// plutôt que d'abandonner le flux à la première anomalie.
fn parcourir(flux: &[u8], voir: &mut impl FnMut(&[u8])) {
    parcourir_a(flux, voir, 0);
}

fn parcourir_a(flux: &[u8], voir: &mut impl FnMut(&[u8]), profondeur: u32) {
    let interne = profondeur > 0;
    let mut o = 0usize;
    while o < flux.len() {
        if flux[o] == 0 {
            o += 1;
            continue;
        }
        let Some((valeur, lus)) = varint(flux, o) else {
            if interne {
                break;
            }
            o += 1;
            continue;
        };
        if valeur <= 3 {
            if interne {
                break;
            }
            o += 1;
            continue;
        }
        let taille = valeur - 3;
        if taille > 65535 || o + taille > flux.len() {
            if interne {
                break;
            }
            o += 1;
            continue;
        }

        let paquet = &flux[o..o + taille];
        let est_groupe =
            paquet.len() > lus + 1 && paquet[lus] == 0xFF && paquet[lus + 1] == 0xFF;

        if est_groupe {
            let supplement = if interne { 0 } else { 1 };
            let fin = (o + taille + supplement).min(flux.len());
            let charge = &flux[o + lus..fin];
            if charge.len() > 6 {
                let taille_decompressee =
                    u32::from_le_bytes([charge[2], charge[3], charge[4], charge[5]]) as usize;
                if taille_decompressee > 0 && taille_decompressee <= 1_000_000 {
                    if let Ok(decompresse) =
                        lz4_flex::block::decompress(&charge[6..], taille_decompressee)
                    {
                        parcourir_a(&decompresse, voir, profondeur + 1);
                    }
                }
            }
            o += taille + supplement;
        } else {
            voir(paquet);
            o += taille;
        }
    }
}

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
            if let Some((_, lus)) = varint(p, 0) {
                if let (Some(a), Some(b)) = (p.get(lus), p.get(lus + 1)) {
                    *opcodes.entry((*a, *b)).or_insert(0usize) += 1;
                }
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
