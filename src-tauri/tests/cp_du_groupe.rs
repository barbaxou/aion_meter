//! Le Combat Power des autres membres du groupe arrive-t-il jusqu'à l'overlay ?
//!
//! Mesuré le 16/10/2026 : Abyss DPS Meter affiche le CP à côté de chaque pseudo
//! (129,2 K, 120,3 K…) ; chez nous seul un membre l'affichait. Ce diagnostic
//! rejoue une capture et imprime ce que le groupe contient à la fin.
//!
//!   A2_REPLAY_CAPTURE=... cargo test --test cp_du_groupe -- --ignored --nocapture

use std::collections::HashMap;
use std::sync::Arc;

use xiiinrv_meter_lib::capture::framing::{walk, walk_inner, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;
use xiiinrv_meter_lib::capture::stream_assembler::StreamAssembler;
use xiiinrv_meter_lib::capture::stream_processor::read_varint;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 { return None; }
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect()
}

#[test]
#[ignore = "diagnostic"]
fn cp_des_membres_du_groupe() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else { return };
    let texte = std::fs::read_to_string(&chemin).expect("capture lisible");
    let storage = Arc::new(DataStorage::new());
    let mut flux: HashMap<String, (StreamAssembler, StreamProcessor)> = HashMap::new();
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') { continue; }
        let b: Vec<&str> = ligne.splitn(3, '|').collect();
        if b.len() != 3 { continue; }
        let Some(d) = decode_hex(b[2]) else { continue };
        let (a, p) = flux.entry(b[1].to_string()).or_insert_with(|| (
            StreamAssembler::new(),
            StreamProcessor::new(storage.clone(), Arc::new(SkillLookup::new()), Arc::new(NpcLookup::new())),
        ));
        a.process_chunk(&d, p);
    }
    let mut membres: Vec<_> = storage.get_party_members().into_iter().collect();
    membres.sort_by_key(|(_, m)| m.slot);
    println!("\n{} membres dans le groupe à la fin de la capture :", membres.len());
    for (nom, m) in &membres {
        println!("  slot {} {:<14} niveau {} équipement {} CP {}", m.slot, nom, m.level, m.gear_score, m.combat_power);
    }
}

fn aplatir(buffer: &[u8], out: &mut Vec<Vec<u8>>, profondeur: usize, interne: bool) {
    if profondeur > 4 { return; }
    let d = if interne { walk_inner(buffer) } else { walk(buffer) };
    for frame in d.frames {
        match frame.kind {
            FrameKind::Packet => out.push(frame.bytes(buffer).to_vec()),
            FrameKind::Bundle => {
                let p = frame.payload(buffer);
                if p.len() < 7 { continue; }
                let taille = u32::from_le_bytes([p[2], p[3], p[4], p[5]]) as usize;
                if taille == 0 || taille > 1_000_000 { continue; }
                if let Ok(int) = lz4_flex::decompress(&p[6..], taille) {
                    aplatir(&int, out, profondeur + 1, true);
                }
            }
        }
    }
}

/// Imprime les plus longs paquets `02 97` (liste du groupe) en hexadécimal.
#[test]
#[ignore = "diagnostic"]
fn paquets_de_la_liste_du_groupe() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else { return };
    let texte = std::fs::read_to_string(&chemin).expect("capture lisible");
    let mut accs: HashMap<String, PacketAccumulator> = HashMap::new();
    let mut vus: Vec<Vec<u8>> = Vec::new();
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') { continue; }
        let b: Vec<&str> = ligne.splitn(3, '|').collect();
        if b.len() != 3 || !b[1].starts_with("Client") { continue; }
        let Some(d) = decode_hex(b[2]) else { continue };
        let acc = accs.entry(b[1].to_string()).or_insert_with(PacketAccumulator::new);
        acc.append(&d);
        let tampon = acc.snapshot().to_vec();
        let mut paquets = Vec::new();
        aplatir(&tampon, &mut paquets, 0, false);
        let w = walk(&tampon);
        if w.consumed > 0 { acc.discard_bytes(w.consumed); }
        for p in paquets {
            let li = read_varint(&p, 0);
            if li.length < 0 { continue; }
            let o = li.length as usize;
            if o + 1 < p.len() && p[o] == 0x02 && p[o + 1] == 0x97 { vus.push(p); }
        }
    }
    println!("
{} paquets 02 97", vus.len());
    vus.sort_by_key(|p| std::cmp::Reverse(p.len()));
    vus.dedup();
    for p in vus.iter().take(3) {
        let hex: String = p.iter().map(|x| format!("{x:02x}")).collect();
        println!("--- {} octets
{hex}", p.len());
    }
}
