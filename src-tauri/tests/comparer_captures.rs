//! Compare les paquets **décompressés** de deux captures sur une même fenêtre.
//!
//! Le 07/10/2026, l'équipement de barbaxou n'arrivait pas, alors qu'un meter
//! tiers le recevait sur la même machine au même instant. Notre décodeur n'est
//! pas en cause : nourri de la capture non filtrée, il rend les 22 pièces avec
//! les bons emplacements et les bons enchantements.
//!
//! Comparer les flux **bruts** ne prouve rien : ils portent des lots LZ4, et
//! deux octets identiques dans des données compressées sont une coïncidence —
//! l'erreur a été commise trois fois dans la même journée. La seule comparaison
//! qui vaille porte sur les paquets une fois décompressés.
//!
//! Ce diagnostic dit, pour chacune des deux captures et sur la même fenêtre
//! horaire : combien de paquets en sortent, de quelles tailles, et quels
//! opcodes les mènent. Si l'une en a moins que l'autre, la perte est dans la
//! capture ou le réassemblage ; si les deux ont les mêmes paquets et qu'un seul
//! opcode manque d'un côté, c'est le découpage.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_CAPTURE_A=... A2_CAPTURE_B=... A2_DE=14:49:40 A2_A=14:50:15 \
//!     cargo test --test comparer_captures -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, walk_inner, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn aplatir_interne(buffer: &[u8], out: &mut Vec<Vec<u8>>, profondeur: usize) {
    if profondeur > 4 {
        return;
    }
    for frame in walk_inner(buffer).frames {
        match frame.kind {
            FrameKind::Packet => out.push(frame.bytes(buffer).to_vec()),
            FrameKind::Bundle => {
                let payload = frame.payload(buffer);
                if payload.len() < 7 {
                    continue;
                }
                let taille =
                    u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;
                if taille == 0 || taille > 1_000_000 {
                    continue;
                }
                if let Ok(interne) = lz4_flex::decompress(&payload[6..], taille) {
                    aplatir_interne(&interne, out, profondeur + 1);
                }
            }
        }
    }
}

fn aplatir(buffer: &[u8], out: &mut Vec<Vec<u8>>, profondeur: usize) {
    if profondeur > 4 {
        return;
    }
    for frame in walk(buffer).frames {
        match frame.kind {
            FrameKind::Packet => out.push(frame.bytes(buffer).to_vec()),
            FrameKind::Bundle => {
                let payload = frame.payload(buffer);
                if payload.len() < 7 {
                    continue;
                }
                let taille =
                    u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;
                if taille == 0 || taille > 1_000_000 {
                    continue;
                }
                if let Ok(interne) = lz4_flex::decompress(&payload[6..], taille) {
                    // `walk_inner`, et non `walk` : c'est ce que fait le meter
                    // pour le contenu décompressé. S'en écarter produisait de
                    // gros blocs fusionnés et faisait croire, le 07/10/2026,
                    // que nos paquets étaient dix fois trop gros.
                    aplatir_interne(&interne, out, profondeur + 1);
                }
            }
        }
    }
}

/// L'en-tête, lu comme `collecte::entete` : varint de longueur, puis l'opcode.
fn opcode(p: &[u8]) -> Option<[u8; 2]> {
    let mut lus = 0usize;
    loop {
        let octet = *p.get(lus)?;
        lus += 1;
        if octet & 0x80 == 0 {
            break;
        }
        if lus >= 5 {
            return None;
        }
    }
    Some([*p.get(lus)?, *p.get(lus + 1)?])
}

struct Bilan {
    paquets: usize,
    octets: usize,
    opcodes: HashMap<[u8; 2], usize>,
    plus_gros: usize,
    lignes: usize,
    /// Les paquets par tranche de taille. Un paquet de jeu dépasse rarement
    /// quelques milliers d'octets — l'inventaire en fait 5 675, les pets 4 094,
    /// la fiche 2 529. Au-delà, c'est presque sûrement un faux paquet produit
    /// par un découpage désaligné, qui en a avalé plusieurs vrais.
    tailles: [usize; 6],
    octets_par_tranche: [usize; 6],
}

/// La tranche de taille d'un paquet : <256, <1k, <4k, <8k, <32k, au-delà.
fn tranche(n: usize) -> usize {
    match n {
        0..=255 => 0,
        256..=1023 => 1,
        1024..=4095 => 2,
        4096..=8191 => 3,
        8192..=32767 => 4,
        _ => 5,
    }
}

fn depouiller(chemin: &str, de: &str, a: &str) -> Bilan {
    let texte = std::fs::read_to_string(chemin).expect("enregistrement lisible");
    let mut flux: HashMap<String, PacketAccumulator> = HashMap::new();
    let mut bilan = Bilan {
        paquets: 0,
        octets: 0,
        opcodes: HashMap::new(),
        plus_gros: 0,
        lignes: 0,
        tailles: [0; 6],
        octets_par_tranche: [0; 6],
    };

    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
        if bouts.len() != 3 {
            continue;
        }
        // La fenêtre se juge sur l'heure du morceau, pas du paquet reconstitué :
        // c'est la seule date dont on dispose, et elle suffit à cadrer.
        let heure = &bouts[0][11..19.min(bouts[0].len())];
        if heure < de || heure > a {
            continue;
        }
        let Some(octets) = decode_hex(bouts[2]) else { continue };
        bilan.lignes += 1;

        let acc = flux
            .entry(bouts[1].to_string())
            .or_insert_with(PacketAccumulator::new);
        acc.append(&octets);
        let consomme = walk(acc.snapshot()).consumed;
        if consomme == 0 {
            continue;
        }
        let complet = acc.snapshot()[..consomme].to_vec();
        acc.discard_bytes(consomme);

        let mut v = Vec::new();
        aplatir(&complet, &mut v, 0);
        for p in v {
            bilan.paquets += 1;
            bilan.octets += p.len();
            bilan.plus_gros = bilan.plus_gros.max(p.len());
            let t = tranche(p.len());
            bilan.tailles[t] += 1;
            bilan.octets_par_tranche[t] += p.len();
            if let Some(op) = opcode(&p) {
                *bilan.opcodes.entry(op).or_default() += 1;
            }
        }
    }
    bilan
}

#[test]
#[ignore = "diagnostic"]
fn comparer_les_paquets_decompresses() {
    let (Ok(a), Ok(b)) = (std::env::var("A2_CAPTURE_A"), std::env::var("A2_CAPTURE_B")) else {
        eprintln!("A2_CAPTURE_A et A2_CAPTURE_B attendus — test ignoré");
        return;
    };
    let de = std::env::var("A2_DE").unwrap_or_else(|_| "00:00:00".to_string());
    let jusqua = std::env::var("A2_A").unwrap_or_else(|_| "23:59:59".to_string());

    let ba = depouiller(&a, &de, &jusqua);
    let bb = depouiller(&b, &de, &jusqua);

    println!("\nfenêtre {de} → {jusqua}");
    for (nom, bilan) in [("A", &ba), ("B", &bb)] {
        println!(
            "  {nom} : {} lignes → {} paquets, {} octets, plus gros {}",
            bilan.lignes, bilan.paquets, bilan.octets, bilan.plus_gros
        );
    }

    const NOMS: [&str; 6] = ["<256", "<1k", "<4k", "<8k", "<32k", ">=32k"];
    println!("
  répartition des tailles de paquets (nombre / octets) :");
    println!("           {}", NOMS.map(|n| format!("{n:>12}")).join(""));
    for (nom, bilan) in [("A", &ba), ("B", &bb)] {
        let n: String = bilan.tailles.iter().map(|v| format!("{v:>12}")).collect();
        let o: String = bilan
            .octets_par_tranche
            .iter()
            .map(|v| format!("{:>11}k", v / 1024))
            .collect();
        println!("  {nom} nb   {n}");
        println!("  {nom} oct  {o}");
    }

    // Les opcodes présents d'un côté et pas de l'autre : c'est là que se loge
    // ce qu'on perd.
    let mut tous: Vec<[u8; 2]> = ba.opcodes.keys().chain(bb.opcodes.keys()).copied().collect();
    tous.sort();
    tous.dedup();
    println!("\n  opcode      A        B    écart");
    let mut ecarts: Vec<([u8; 2], usize, usize)> = tous
        .into_iter()
        .map(|op| {
            (
                op,
                ba.opcodes.get(&op).copied().unwrap_or(0),
                bb.opcodes.get(&op).copied().unwrap_or(0),
            )
        })
        .collect();
    // Les plus gros écarts d'abord : c'est ce qu'on cherche.
    ecarts.sort_by_key(|(_, na, nb)| std::cmp::Reverse(na.abs_diff(*nb)));
    for (op, na, nb) in ecarts.iter().take(20) {
        let marque = if *na == 0 || *nb == 0 { "  <<<" } else { "" };
        println!(
            "  {:02X} {:02X}  {:>6}  {:>6}  {:>6}{}",
            op[0],
            op[1],
            na,
            nb,
            (*na as i64 - *nb as i64),
            marque
        );
    }
}
