//! Testes de estabilidade e desempenho (só compilados em teste).
//!
//! * Robustez: arquivos válidos de cada formato exportável são truncados, têm bytes alterados e
//!   são substituídos por lixo; a leitura nunca pode entrar em pânico (erro é aceitável).
//! * Desempenho: medições com `cargo test --release -- --ignored --nocapture perf_`; os números
//!   ficam em `docs/PERFORMANCE.md` e não são verificados por limites rígidos.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::document::{Document, Layer};
use crate::filter::{self, Filter};
use crate::formats::{self, ExportOpts, Format};
use crate::{compose, orva};

/// Gerador pseudoaleatório determinístico (xorshift) para reproduzir falhas.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn sample_doc() -> Document {
    let mut d = Document::new("amostra", 48, 32, Some([30, 90, 160, 255])).unwrap();
    let mut top = Layer::transparent("Topo", 48, 32);
    for y in 8..24 {
        for x in 10..38 {
            top.set_pixel(x, y, [230, 60, 40, 200]);
        }
    }
    d.layers.push(top);
    d
}

/// Lê `bytes` como `ext`; devolve `false` se houve pânico.
fn read_survives(bytes: &[u8], ext: &str) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        if ext == "orva" {
            let _ = orva::read(bytes);
        } else {
            let _ = formats::import(bytes, "x", ext);
        }
    }))
    .is_ok()
}

fn mutations(valid: &[u8], rng: &mut Rng) -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new(), vec![0], valid[..valid.len() / 2].to_vec()];
    for cut in [1, 4, 16, 64] {
        if valid.len() > cut {
            out.push(valid[..valid.len() - cut].to_vec());
        }
    }
    for _ in 0..40 {
        let mut m = valid.to_vec();
        for _ in 0..1 + rng.below(6) {
            let i = rng.below(m.len());
            m[i] = rng.next() as u8;
        }
        out.push(m);
    }
    for _ in 0..4 {
        out.push((0..1 + rng.below(512)).map(|_| rng.next() as u8).collect());
    }
    out
}

#[test]
fn corrupted_files_never_panic_in_any_format() {
    let doc = sample_doc();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cases = 0usize;
    let mut sources: Vec<(String, Vec<u8>)> = vec![("orva".into(), orva::write(&doc).unwrap())];
    for f in Format::exportable() {
        if let Ok(e) = formats::export(&doc, f, &ExportOpts::default()) {
            sources.push((f.default_ext().to_string(), e.bytes));
        }
    }
    assert!(sources.len() >= 8, "vários formatos exercitados");
    for (ext, valid) in &sources {
        for m in mutations(valid, &mut rng) {
            assert!(read_survives(&m, ext), "pânico lendo .{ext} corrompido");
            cases += 1;
        }
        // O conteúdo errado para a extensão também não pode derrubar o leitor.
        for other in ["png", "psd", "ora", "pcraft", "svg", "orva"] {
            assert!(read_survives(valid, other), ".{ext} lido como .{other}");
            cases += 1;
        }
    }
    assert!(cases > 400, "{cases}");
}

#[test]
fn svg_and_text_inputs_that_are_hostile_do_not_panic() {
    let bombs: [&[u8]; 4] = [
        b"<svg xmlns='http://www.w3.org/2000/svg' width='99999999' height='99999999'/>",
        b"<svg xmlns='http://www.w3.org/2000/svg'><use href='#a'/></svg>",
        b"<?xml version='1.0'?><!DOCTYPE x [<!ENTITY a 'aaaa'>]><svg>&a;&a;</svg>",
        b"\xff\xfe\x00\x00",
    ];
    for b in bombs {
        assert!(read_survives(b, "svg"));
    }
}

fn busy_doc(w: u32, h: u32, layers: usize) -> Document {
    let mut d = Document::new("perf", w, h, Some([200, 200, 200, 255])).unwrap();
    for i in 0..layers {
        let mut l = Layer::transparent(format!("C{i}"), w, h);
        let off = (i as u32 * 97) % (w / 2);
        for y in (h / 4)..(3 * h / 4) {
            for x in off..(off + w / 3) {
                l.set_pixel(x, y, [(i * 40) as u8, 100, 200, 180]);
            }
        }
        d.layers.push(l);
    }
    d
}

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("PERF {label}: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    r
}

#[test]
#[ignore = "medição; rode com --release -- --ignored --nocapture perf_"]
fn perf_compose_filter_and_files() {
    let d = busy_doc(4000, 3000, 6);
    time("compor 4000x3000 com 7 camadas", || compose::flatten(&d));
    let flat = compose::flatten(&d);
    let cancel = AtomicBool::new(false);
    time("desfoque gaussiano r=8 em 4000x3000", || {
        filter::apply(
            &Filter::GaussianBlur { radius: 8.0 },
            &flat,
            4000,
            3000,
            &cancel,
        )
    });
    let small = busy_doc(1920, 1080, 4);
    let orva_bytes = time("gravar .orva 1920x1080 com 5 camadas", || {
        orva::write(&small).unwrap()
    });
    time("ler .orva", || orva::read(&orva_bytes).unwrap());
    for f in [Format::Png, Format::Jpeg, Format::Webp] {
        let e = time(&format!("exportar {:?} 1920x1080", f), || {
            formats::export(&small, f, &ExportOpts::default()).unwrap()
        });
        println!("PERF   tamanho {:?}: {} KiB", f, e.bytes.len() / 1024);
    }
}
