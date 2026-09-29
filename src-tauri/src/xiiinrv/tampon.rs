//! Récupérer les paquets d'avant le verrouillage du port.
//!
//! LE PROBLÈME
//!   A2Tools ne connaît pas d'avance le port du jeu : il l'apprend en voyant
//!   passer assez de paquets de combat, et tant qu'il ne le connaît pas il jette
//!   tout ce qui n'y ressemble pas. Or l'inventaire et le Combat Power arrivent
//!   précisément à cet instant-là, à l'entrée en jeu, dans la seconde qui précède
//!   le verrouillage. Ils étaient donc jetés avant d'arriver jusqu'à nous.
//!
//! DEUX ESSAIS AVANT CELUI-CI, tous deux dans le filtre d'A2Tools :
//!   1. ne laisser passer que les morceaux portant l'un de nos opcodes —
//!      insuffisant : un gros paquet est découpé par le réseau, seul le premier
//!      morceau porte l'opcode et les suivants étaient jetés ;
//!   2. laisser passer tout le flux dès qu'un de nos opcodes y apparaît — les
//!      octets se retrouvent par hasard sur d'autres connexions locales : sur une
//!      vraie session nous avons retenu les flux 55987 et 64270 alors que le jeu
//!      parlait sur 61944. On filtrait le mauvais trafic.
//!
//! CE QU'ON FAIT MAINTENANT
//!   Plus rien ne change dans le filtre d'A2Tools : il reste exactement celui
//!   d'origine. On se contente de **garder une copie** des morceaux qui passent
//!   devant nous tant que le port n'est pas verrouillé, puis, au verrouillage, de
//!   relire nous-mêmes **le flux retenu par A2Tools** — celui du jeu, sans
//!   ambiguïté. Après quoi le tampon est vidé : le chemin normal prend la suite.

use parking_lot::Mutex;
use std::sync::OnceLock;
use tracing::info;

use super::collecte::{lecture_ouverte, observer};

// Plafonds : si le port ne se verrouille jamais, ce tampon ne doit pas grossir
// indéfiniment. Ils sont posés **par sens de connexion** et non seulement au
// total : sur une machine qui passe par un relais, plusieurs connexions locales
// défilent avant le verrouillage, et un unique plafond global pourrait être
// rempli par ce bruit avant même que le flux du jeu n'apparaisse. L'entrée en
// jeu tient très largement dans un sens (quelques centaines de kilo-octets).
const PLAFOND_PAR_SENS: usize = 3 * 1024 * 1024;
const PLAFOND_TOTAL: usize = 12 * 1024 * 1024;
const SENS_SUIVIS_AU_PLUS: usize = 24;

/// Port d'origine et port de destination, **dans cet ordre**. Avant le
/// verrouillage, les deux sens de la connexion défilent ici : les mêler dans un
/// seul tampon rendrait le découpage illisible. Chaque sens a donc le sien, et
/// on ne relit que celui qui va du serveur vers le client.
type Flux = (u16, u16);

static TAMPON: OnceLock<Mutex<Vec<(Flux, Vec<u8>)>>> = OnceLock::new();

fn tampon() -> &'static Mutex<Vec<(Flux, Vec<u8>)>> {
    TAMPON.get_or_init(|| Mutex::new(Vec::new()))
}

/// Garde une copie d'un morceau vu avant le verrouillage du port.
/// Ne fait rien si le partage n'est pas activé.
pub fn mettre_de_cote(origine: u16, destination: u16, donnees: &[u8]) {
    if !lecture_ouverte() || donnees.is_empty() {
        return;
    }
    let cle = (origine, destination);
    let mut tampon = tampon().lock();
    let total: usize = tampon.iter().map(|(_, o)| o.len()).sum();
    if total + donnees.len() > PLAFOND_TOTAL {
        return;
    }
    match tampon.iter_mut().find(|(k, _)| *k == cle) {
        Some((_, octets)) => {
            if octets.len() + donnees.len() <= PLAFOND_PAR_SENS {
                octets.extend_from_slice(donnees);
            }
        }
        None => {
            if tampon.len() < SENS_SUIVIS_AU_PLUS {
                tampon.push((cle, donnees.to_vec()));
            }
        }
    }
}

/// Le port vient d'être verrouillé : `origine` est le port du jeu. On relit le
/// sens serveur → client, le seul qui circule en clair, puis on vide tout.
pub fn relire(origine: u16, destination: u16) {
    let cle = (origine, destination);
    let garde = {
        let mut tampon = tampon().lock();
        let garde = tampon
            .iter()
            .position(|(k, _)| *k == cle)
            .map(|i| tampon.swap_remove(i).1);
        tampon.clear();
        garde
    };
    let Some(octets) = garde else {
        return;
    };
    if !lecture_ouverte() {
        return;
    }
    // Le tampon commence presque toujours au milieu d'un paquet : ses premiers
    // octets sont la fin du précédent. Sans ce recalage, le découpeur se
    // resynchronise octet par octet et peut fabriquer un paquet qui n'existe
    // pas — c'est ainsi qu'un Combat Power de 77 148 est remonté au site le
    // 29/09 alors que le jeu en affichait 132 462.
    let Some(debut) = debut_aligne(&octets) else {
        info!(
            "XIII NRV : flux {:?} verrouillé, {} octets gardés mais aucun début              de paquet reconnaissable : rien n'est relu",
            cle,
            octets.len()
        );
        return;
    };
    let mut paquets = 0usize;
    parcourir(&octets[debut..], &mut |paquet| {
        paquets += 1;
        observer(paquet);
    });
    info!(
        "XIII NRV : flux {:?} verrouillé, {} octets gardés relus en {} paquets          (début aligné à {})",
        cle,
        octets.len(),
        paquets,
        debut
    );
}

/// Le partage vient d'être coupé : on ne garde rien.
pub fn oublier() {
    tampon().lock().clear();
}

// ---------------------------------------------------------------------------
// Découpage du flux
// ---------------------------------------------------------------------------

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

/// Le premier endroit du tampon où commence vraiment un paquet.
///
/// Un tampon commence là où le meter a commencé à écouter, donc presque toujours
/// au milieu d'un paquet : ses premiers octets sont la fin du précédent. Parti de
/// là, le découpage se décale, et un décalage ne se rattrape pas toujours : une
/// longueur lue de travers fait sauter par-dessus un groupe compressé entier, et
/// tout son contenu est perdu. C'est ce qui est arrivé le 29/09/2026 — le
/// Combat Power défile en trois paquets et le site n'a reçu que le premier.
///
/// Ce qu'on prend comme preuve d'un bon départ : **une décompression qui
/// réussit**. Le flux du jeu est fait pour l'essentiel de groupes compressés, et
/// un découpage décalé ne produit presque jamais un bloc valide de la taille
/// annoncée — alors qu'un découpage juste les enchaîne tous. Compter les octets
/// « expliqués » ne suffisait pas : plusieurs départs y arrivent aussi bien, et
/// on retenait le mauvais.
///
/// Renvoie `None` si aucune position ne convainc : mieux vaut ne rien relire que
/// relire de travers.
const FENETRE_DE_CONTROLE: usize = 64 * 1024;
const CANDIDATS_AU_PLUS: usize = 4096;
const GROUPES_POUR_ETRE_SUR: usize = 4;
const MINIMUM_CREDIBLE: usize = 32;

/// Ce qu'un départ explique : combien de groupes compressés s'y décompressent, et
/// combien d'octets s'enchaînent sans le moindre rattrapage.
fn qualite(flux: &[u8], debut: usize) -> (usize, usize) {
    let limite = (debut + FENETRE_DE_CONTROLE).min(flux.len());
    let mut o = debut;
    let mut groupes = 0usize;
    while o < limite {
        // Comme le découpeur : un octet nul est du remplissage, on passe.
        if flux[o] == 0 {
            o += 1;
            continue;
        }
        let Some((valeur, lus)) = varint(flux, o) else {
            break;
        };
        if valeur <= 3 {
            break;
        }
        let taille = valeur - 3;
        if taille > 65535 || o + taille > flux.len() {
            break;
        }
        let paquet = &flux[o..o + taille];
        if paquet.len() > lus + 1 && paquet[lus] == 0xFF && paquet[lus + 1] == 0xFF {
            // Un groupe compressé occupe un octet de plus au premier niveau.
            let fin = (o + taille + 1).min(flux.len());
            let charge = &flux[o + lus..fin];
            if charge.len() <= 6 {
                break;
            }
            let attendu =
                u32::from_le_bytes([charge[2], charge[3], charge[4], charge[5]]) as usize;
            if attendu == 0
                || attendu > 1_000_000
                || lz4_flex::block::decompress(&charge[6..], attendu).is_err()
            {
                break; // un groupe qui ne se décompresse pas : ce départ est faux
            }
            groupes += 1;
            if groupes >= GROUPES_POUR_ETRE_SUR {
                return (groupes, limite - debut); // assez vu
            }
            o += taille + 1;
        } else {
            o += taille;
        }
    }
    (groupes, o - debut)
}

pub fn debut_aligne(flux: &[u8]) -> Option<usize> {
    let mut meilleur: Option<(usize, usize, usize)> = None; // (groupes, octets, début)
    for debut in 0..flux.len().min(CANDIDATS_AU_PLUS) {
        let (groupes, octets) = qualite(flux, debut);
        if groupes >= GROUPES_POUR_ETRE_SUR {
            return Some(debut);
        }
        // Un départ qui explique le tampon jusqu'au bout convient aussi : les
        // tampons courts n'ont pas toujours un seul groupe compressé.
        if octets > 0 && debut + octets >= flux.len() && groupes > 0 {
            return Some(debut);
        }
        if meilleur.map_or(true, |(g, o, _)| (groupes, octets) > (g, o)) {
            meilleur = Some((groupes, octets, debut));
        }
    }
    meilleur
        .filter(|(groupes, octets, debut)| {
            *groupes > 0 || (*octets >= MINIMUM_CREDIBLE && debut + octets >= flux.len())
        })
        .map(|(_, _, debut)| debut)
}

/// Même découpage que `consume_stream` d'A2Tools : longueur varint (moins 3) et
/// groupes compressés `FF FF`. Deux détails qui comptent : au premier niveau un
/// groupe occupe **un octet de plus**, et on se resynchronise octet par octet
/// plutôt que d'abandonner le flux à la première anomalie — un tampon commence
/// rarement pile au début d'un paquet.
pub fn parcourir(flux: &[u8], voir: &mut impl FnMut(&[u8])) {
    parcourir_a(flux, voir, 0);
}

fn parcourir_a(flux: &[u8], voir: &mut impl FnMut(&[u8]), profondeur: u32) {
    if profondeur > 4 {
        return;
    }
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
        let est_groupe = paquet.len() > lus + 1 && paquet[lus] == 0xFF && paquet[lus + 1] == 0xFF;

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
