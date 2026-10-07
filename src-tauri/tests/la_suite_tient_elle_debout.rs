//! Après un paquet correctement découpé, la suite forme-t-elle un en-tête valide ?
//!
//! Mesuré le 07/10/2026 : notre découpage fusionne. Sur une même fenêtre de 90
//! secondes, nous rendions 1 315 paquets là où un meter tiers en rendait 5 782,
//! et 69 % de nos octets tenaient dans dix-sept gros blocs. Un paquet porteur
//! du motif `0E 00 36` en contenait 3,2 en moyenne, contre exactement 1 chez
//! lui : nos paquets avalent leurs voisins, et tout ce qu'ils avalent est perdu
//! — on ne lit que l'opcode du premier.
//!
//! Ces fusions ne déclenchent aucune reprise : une longueur fausse mais
//! *plausible* passe tous les contrôles.
//!
//! Le motif ne peut pas servir d'ancre — il est à la fois un paquet de
//! battement et un terminateur d'enregistrement à l'intérieur d'un paquet de
//! dégâts, et s'en servir coûtait 5,5 % des coups.
//!
//! D'où ce critère, qui ne dépend d'aucun motif : **si la longueur est bonne,
//! les octets qui suivent le paquet forment eux aussi un en-tête plausible.**
//! C'est la corroboration que ce projet emploie déjà ailleurs — le Combat Power
//! retenu seulement si un second paquet le confirme.
//!
//! Ce diagnostic mesure si le critère discrimine : il faut qu'il soit vrai pour
//! la quasi-totalité des petits paquets, et faux pour les gros blocs. S'il
//! rejette aussi les petits, il ne vaut rien.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test la_suite_tient_elle_debout -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;
use xiiinrv_meter_lib::capture::stream_processor::read_varint;

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// Les octets à `depuis` forment-ils un en-tête de paquet plausible ?
///
/// Mêmes règles que `framing::walk` : un varint de longueur lisible, et une
/// taille physique (longueur moins trois) ni nulle ni délirante.
fn entete_plausible(buffer: &[u8], depuis: usize) -> bool {
    if depuis >= buffer.len() {
        // La fin du tampon n'infirme rien : la suite n'est pas encore arrivée.
        return true;
    }
    // Le remplissage à zéro est légitime entre deux paquets.
    let mut i = depuis;
    while i < buffer.len() && buffer[i] == 0x00 {
        i += 1;
    }
    if i >= buffer.len() {
        return true;
    }
    let li = read_varint(buffer, i);
    if li.length <= 0 || li.value <= 3 {
        return false;
    }
    let taille = (li.value - 3) as usize;
    taille > 0 && taille <= 65535
}

#[test]
#[ignore = "diagnostic"]
fn la_suite_tient_elle_debout() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    let mut flux: HashMap<String, PacketAccumulator> = HashMap::new();
    // Par tranche de taille : (paquets, dont la suite tient debout).
    let mut par_tranche: [(usize, usize); 6] = [(0, 0); 6];
    const NOMS: [&str; 6] = ["<256", "<1k", "<4k", "<8k", "<32k", ">=32k"];
    let tranche = |n: usize| match n {
        0..=255 => 0,
        256..=1023 => 1,
        1024..=4095 => 2,
        4096..=8191 => 3,
        8192..=32767 => 4,
        _ => 5,
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
        let Some(octets) = decode_hex(bouts[2]) else { continue };
        let acc = flux
            .entry(bouts[1].to_string())
            .or_insert_with(PacketAccumulator::new);
        acc.append(&octets);

        let tampon = acc.snapshot().to_vec();
        let decoupe = walk(&tampon);
        for frame in &decoupe.frames {
            // Seuls les paquets simples : un lot a sa propre arithmétique.
            if frame.kind != FrameKind::Packet {
                continue;
            }
            let taille = frame.end - frame.start;
            let t = tranche(taille);
            par_tranche[t].0 += 1;
            if entete_plausible(&tampon, frame.end) {
                par_tranche[t].1 += 1;
            }
        }
        if decoupe.consumed > 0 {
            acc.discard_bytes(decoupe.consumed);
        }
    }

    println!("\n{chemin}");
    println!("\n  taille    paquets   suite valide   taux");
    for (i, nom) in NOMS.iter().enumerate() {
        let (total, bons) = par_tranche[i];
        if total == 0 {
            continue;
        }
        println!(
            "  {nom:<8} {total:>8} {bons:>14}   {:.1} %",
            100.0 * bons as f64 / total as f64
        );
    }
    println!(
        "\n  Le critère ne vaut que s'il est presque toujours vrai pour les petits\n  \
         paquets et souvent faux pour les gros. Sinon il rejetterait du bon."
    );
}
