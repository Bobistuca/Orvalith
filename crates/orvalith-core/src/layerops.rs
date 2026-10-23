//! Operações estruturais sobre a pilha de camadas (puras: devolvem a nova lista).
//!
//! Invariante mantido por `normalize`: a lista está em pré-ordem — cada grupo é seguido
//! imediatamente por seus descendentes — e a ordem relativa dos irmãos é a ordem da lista.
//! O resultado de cada operação é aplicado ao documento por `edit::LayersEdit`.

use std::collections::{HashMap, HashSet};

use crate::compose;
use crate::document::{descendants, Document, Layer, LayerId};

/// Resultado de uma operação estrutural.
pub struct Outcome {
    pub layers: Vec<Layer>,
    /// Nova seleção sugerida.
    pub select: Vec<LayerId>,
    /// Nova camada ativa sugerida.
    pub active: Option<LayerId>,
}

/// Reconstrói a lista em pré-ordem; órfãos e ciclos viram itens da raiz.
pub fn normalize(mut layers: Vec<Layer>) -> Vec<Layer> {
    let ids: HashSet<LayerId> = layers
        .iter()
        .filter(|l| l.is_group())
        .map(|l| l.id)
        .collect();
    for l in &mut layers {
        if l.parent.is_some_and(|p| !ids.contains(&p) || p == l.id) {
            l.parent = None;
        }
    }
    let mut out = Vec::with_capacity(layers.len());
    let mut seen = HashSet::new();
    fn emit(
        parent: Option<LayerId>,
        src: &[Layer],
        out: &mut Vec<Layer>,
        seen: &mut HashSet<LayerId>,
    ) {
        for l in src.iter().filter(|l| l.parent == parent) {
            if !seen.insert(l.id) {
                continue;
            }
            out.push(l.clone());
            if l.is_group() {
                emit(Some(l.id), src, out, seen);
            }
        }
    }
    emit(None, &layers, &mut out, &mut seen);
    // Qualquer resto (ciclos) vai para a raiz para nunca perder camadas.
    for l in &layers {
        if !seen.contains(&l.id) {
            let mut l = l.clone();
            l.parent = None;
            seen.insert(l.id);
            out.push(l);
        }
    }
    out
}

/// Remove da seleção os IDs cujo ancestral também está selecionado.
fn top_level(layers: &[Layer], ids: &[LayerId]) -> Vec<LayerId> {
    let set: HashSet<LayerId> = ids.iter().copied().collect();
    layers
        .iter()
        .filter(|l| set.contains(&l.id))
        .filter(|l| {
            let mut cur = l.parent;
            while let Some(p) = cur {
                if set.contains(&p) {
                    return false;
                }
                cur = layers.iter().find(|x| x.id == p).and_then(|x| x.parent);
            }
            true
        })
        .map(|l| l.id)
        .collect()
}

fn subtree(layers: &[Layer], id: LayerId) -> Vec<Layer> {
    let mut ids = vec![id];
    ids.extend(descendants(layers, id));
    layers
        .iter()
        .filter(|l| ids.contains(&l.id))
        .cloned()
        .collect()
}

/// Copia subárvores com IDs novos (para duplicar/colar). Raízes ficam com `root_parent`.
fn clone_subtrees(nodes: &[Layer], roots: &[LayerId], root_parent: Option<LayerId>) -> Vec<Layer> {
    let mut map: HashMap<LayerId, LayerId> = HashMap::new();
    let mut out = Vec::new();
    for l in nodes {
        let mut c = l.clone();
        c.id = LayerId::fresh();
        map.insert(l.id, c.id);
        out.push(c);
    }
    for (c, orig) in out.iter_mut().zip(nodes) {
        c.parent = if roots.contains(&orig.id) {
            root_parent
        } else {
            orig.parent.and_then(|p| map.get(&p).copied())
        };
    }
    out
}

/// Copia as subárvores selecionadas para a área de transferência interna.
pub fn copy_nodes(doc: &Document, ids: &[LayerId]) -> Vec<Layer> {
    let roots = top_level(&doc.layers, ids);
    let mut nodes = Vec::new();
    for r in &roots {
        nodes.extend(subtree(&doc.layers, *r));
    }
    // Mantém os IDs originais: `paste` cria IDs novos.
    let mut out = nodes;
    for l in &mut out {
        if roots.contains(&l.id) {
            l.parent = None;
        }
    }
    out
}

/// Duplica camadas/grupos (com filhos), colocando cada cópia logo acima do original.
pub fn duplicate(doc: &Document, ids: &[LayerId]) -> Option<Outcome> {
    let roots = top_level(&doc.layers, ids);
    if roots.is_empty() {
        return None;
    }
    let mut layers = doc.layers.clone();
    let mut select = Vec::new();
    for r in &roots {
        let nodes = subtree(&layers, *r);
        let parent = nodes[0].parent;
        let mut copies = clone_subtrees(&nodes, &[*r], parent);
        copies[0].name = format!("{} cópia", copies[0].name);
        select.push(copies[0].id);
        // Insere logo após o bloco original (pré-ordem), como irmão imediatamente acima.
        let end = layers
            .iter()
            .position(|l| l.id == *r)
            .map_or(layers.len(), |i| i + nodes.len());
        for (k, c) in copies.into_iter().enumerate() {
            layers.insert(end + k, c);
        }
    }
    let layers = normalize(layers);
    Some(Outcome {
        active: select.last().copied(),
        select,
        layers,
    })
}

/// Nova camada transparente logo acima de `above` (mesmo pai), ou no topo da raiz.
pub fn new_layer(doc: &Document, above: Option<LayerId>, name: String) -> Outcome {
    let mut layer = Layer::transparent(name, doc.width, doc.height);
    let (parent, at) = match above.and_then(|id| doc.layer(id)) {
        Some(a) => (
            a.parent,
            doc.index_of(a.id).unwrap_or(0) + 1 + descendants(&doc.layers, a.id).len(),
        ),
        None => (None, doc.layers.len()),
    };
    layer.parent = parent;
    let id = layer.id;
    let mut layers = doc.layers.clone();
    layers.insert(at.min(layers.len()), layer);
    Outcome {
        layers: normalize(layers),
        select: vec![id],
        active: Some(id),
    }
}

/// Exclui camadas/grupos selecionados (e descendentes).
pub fn delete(doc: &Document, ids: &[LayerId]) -> Option<Outcome> {
    let roots = top_level(&doc.layers, ids);
    if roots.is_empty() {
        return None;
    }
    let mut gone: HashSet<LayerId> = roots.iter().copied().collect();
    for r in &roots {
        gone.extend(descendants(&doc.layers, *r));
    }
    let first_idx = doc.layers.iter().position(|l| gone.contains(&l.id))?;
    let layers: Vec<Layer> = doc
        .layers
        .iter()
        .filter(|l| !gone.contains(&l.id))
        .cloned()
        .collect();
    let active = layers
        .get(
            first_idx
                .saturating_sub(1)
                .min(layers.len().saturating_sub(1)),
        )
        .map(|l| l.id);
    Some(Outcome {
        select: active.into_iter().collect(),
        active,
        layers,
    })
}

/// Agrupa as camadas selecionadas (ou cria um grupo vazio acima de `fallback_above`).
pub fn group(doc: &Document, ids: &[LayerId], fallback_above: Option<LayerId>) -> Outcome {
    let roots = top_level(&doc.layers, ids);
    let mut g = Layer::group("Grupo");
    let mut layers = doc.layers.clone();
    if roots.is_empty() {
        let (parent, at) = match fallback_above.and_then(|id| doc.layer(id)) {
            Some(a) => {
                let end =
                    doc.index_of(a.id).unwrap_or(0) + 1 + descendants(&doc.layers, a.id).len();
                (a.parent, end)
            }
            None => (None, layers.len()),
        };
        g.parent = parent;
        let gid = g.id;
        layers.insert(at.min(layers.len()), g);
        return Outcome {
            layers: normalize(layers),
            select: vec![gid],
            active: Some(gid),
        };
    }
    let top = roots
        .iter()
        .filter_map(|r| doc.index_of(*r).map(|i| (i, *r)))
        .max()
        .map(|(_, r)| r)
        .unwrap_or(roots[0]);
    g.parent = doc.layer(top).and_then(|l| l.parent);
    let gid = g.id;
    let at = doc.index_of(top).unwrap_or(0);
    layers.insert(at, g);
    for l in &mut layers {
        if roots.contains(&l.id) {
            l.parent = Some(gid);
        }
    }
    Outcome {
        layers: normalize(layers),
        select: vec![gid],
        active: Some(gid),
    }
}

/// Desfaz um grupo: filhos sobem para o pai do grupo, na posição do grupo.
pub fn ungroup(doc: &Document, gid: LayerId) -> Option<Outcome> {
    let g = doc.layer(gid).filter(|l| l.is_group())?;
    let mut layers = doc.layers.clone();
    let direct: Vec<LayerId> = layers
        .iter()
        .filter(|l| l.parent == Some(gid))
        .map(|l| l.id)
        .collect();
    for l in &mut layers {
        if l.parent == Some(gid) {
            l.parent = g.parent;
        }
    }
    layers.retain(|l| l.id != gid);
    Some(Outcome {
        active: direct.last().copied(),
        select: direct,
        layers: normalize(layers),
    })
}

/// Move a camada/grupo uma posição entre os irmãos. `up` = para cima na pilha.
pub fn move_sibling(doc: &Document, id: LayerId, up: bool) -> Option<Outcome> {
    let me = doc.layer(id)?;
    let sibs: Vec<LayerId> = doc
        .layers
        .iter()
        .filter(|l| l.parent == me.parent)
        .map(|l| l.id)
        .collect();
    let k = sibs.iter().position(|s| *s == id)?;
    let other = if up {
        sibs.get(k + 1).copied()?
    } else {
        sibs.get(k.checked_sub(1)?).copied()?
    };
    let a = subtree(&doc.layers, id);
    let b = subtree(&doc.layers, other);
    let mut layers: Vec<Layer> = doc
        .layers
        .iter()
        .filter(|l| !a.iter().any(|x| x.id == l.id))
        .cloned()
        .collect();
    let b_last = b.last()?.id;
    let b_first = b.first()?.id;
    let at = if up {
        layers.iter().position(|l| l.id == b_last)? + 1
    } else {
        layers.iter().position(|l| l.id == b_first)?
    };
    for (i, l) in a.into_iter().enumerate() {
        layers.insert(at + i, l);
    }
    Some(Outcome {
        select: vec![id],
        active: Some(id),
        layers: normalize(layers),
    })
}

/// Composita as subárvores `roots` isoladamente (fundo transparente) em pixels do tamanho do documento.
fn composite_nodes(doc: &Document, roots: &[LayerId]) -> Vec<u8> {
    let mut nodes = Vec::new();
    for r in roots {
        nodes.extend(subtree(&doc.layers, *r));
    }
    for l in &mut nodes {
        if roots.contains(&l.id) {
            l.parent = None;
        }
    }
    let mut tmp = Document::from_parts(String::new(), doc.width, doc.height, doc.ppi, nodes);
    tmp.layers = normalize(std::mem::take(&mut tmp.layers));
    compose::flatten(&tmp)
}

fn merged_layer(doc: &Document, name: String, roots: &[LayerId]) -> Layer {
    Layer::from_pixels(name, doc.width, doc.height, composite_nodes(doc, roots))
}

/// Mescla as camadas/grupos selecionados numa só camada Normal (posição da mais alta).
/// A mesclagem ocorre sobre transparência: modos de mesclagem de cada item valem apenas
/// entre os itens mesclados.
pub fn merge_selected(doc: &Document, ids: &[LayerId]) -> Option<Outcome> {
    let roots = top_level(&doc.layers, ids);
    if roots.len() < 2 {
        return None;
    }
    let top = roots
        .iter()
        .filter_map(|r| doc.index_of(*r).map(|i| (i, *r)))
        .max()?
        .1;
    let name = doc.layer(top)?.name.clone();
    finish_merge(doc, &roots, top, name)
}

/// Mescla todas as camadas visíveis da raiz; as ocultas permanecem.
pub fn merge_visible(doc: &Document) -> Option<Outcome> {
    let roots: Vec<LayerId> = doc
        .layers
        .iter()
        .filter(|l| l.parent.is_none() && l.visible)
        .map(|l| l.id)
        .collect();
    if roots.len() < 2 {
        return None;
    }
    let top = *roots.last()?;
    let name = doc.layer(top)?.name.clone();
    finish_merge(doc, &roots, top, name)
}

fn finish_merge(doc: &Document, roots: &[LayerId], top: LayerId, name: String) -> Option<Outcome> {
    let mut merged = merged_layer(doc, name, roots);
    merged.parent = doc.layer(top)?.parent;
    let mut gone: HashSet<LayerId> = roots.iter().copied().collect();
    for r in roots {
        gone.extend(descendants(&doc.layers, *r));
    }
    let top_idx = doc.index_of(top)?;
    let mut layers = Vec::new();
    let mut inserted = false;
    for (i, l) in doc.layers.iter().enumerate() {
        if i == top_idx && !inserted {
            layers.push(merged.clone());
            inserted = true;
        }
        if !gone.contains(&l.id) {
            layers.push(l.clone());
        }
    }
    let id = merged.id;
    Some(Outcome {
        layers: normalize(layers),
        select: vec![id],
        active: Some(id),
    })
}

/// Achata a imagem numa única camada; camadas ocultas são descartadas.
pub fn flatten_all(doc: &Document) -> Option<Outcome> {
    let single = doc.layers.len() == 1 && !doc.layers[0].is_group() && doc.layers[0].visible;
    if doc.layers.is_empty() || single {
        return None;
    }
    let name = doc
        .layers
        .iter()
        .find(|l| l.parent.is_none() && !l.is_group())
        .map_or_else(|| "Fundo".to_string(), |l| l.name.clone());
    let layer = Layer::from_pixels(name, doc.width, doc.height, compose::flatten(doc));
    let id = layer.id;
    Some(Outcome {
        layers: vec![layer],
        select: vec![id],
        active: Some(id),
    })
}

/// Cola camadas copiadas (IDs novos) acima do item ativo; conteúdo é centralizado se o
/// tamanho do documento de destino for diferente.
pub fn paste(doc: &Document, clip: &[Layer], above: Option<LayerId>) -> Option<Outcome> {
    if clip.is_empty() {
        return None;
    }
    let roots: Vec<LayerId> = clip
        .iter()
        .filter(|l| l.parent.is_none_or(|p| !clip.iter().any(|c| c.id == p)))
        .map(|l| l.id)
        .collect();
    let (parent, at) = match above.and_then(|id| doc.layer(id)) {
        Some(a) => (
            a.parent,
            doc.index_of(a.id).unwrap_or(0) + 1 + descendants(&doc.layers, a.id).len(),
        ),
        None => (None, doc.layers.len()),
    };
    let mut copies = clone_subtrees(clip, &roots, parent);
    for c in &mut copies {
        if c.is_group() {
            continue;
        }
        if c.width != doc.width || c.height != doc.height {
            *c = recentre(c, doc.width, doc.height);
        }
    }
    let select: Vec<LayerId> = copies
        .iter()
        .filter(|c| c.parent == parent)
        .map(|c| c.id)
        .collect();
    let mut layers = doc.layers.clone();
    for (k, c) in copies.into_iter().enumerate() {
        layers.insert((at + k).min(layers.len()), c);
    }
    Some(Outcome {
        active: select.last().copied(),
        select,
        layers: normalize(layers),
    })
}

fn recentre(src: &Layer, w: u32, h: u32) -> Layer {
    let mut dst = vec![0u8; w as usize * h as usize * 4];
    let ox = (i64::from(w) - i64::from(src.width)) / 2;
    let oy = (i64::from(h) - i64::from(src.height)) / 2;
    for y in 0..i64::from(src.height) {
        let ty = y + oy;
        if ty < 0 || ty >= i64::from(h) {
            continue;
        }
        for x in 0..i64::from(src.width) {
            let tx = x + ox;
            if tx < 0 || tx >= i64::from(w) {
                continue;
            }
            let s = (y as usize * src.width as usize + x as usize) * 4;
            let d = (ty as usize * w as usize + tx as usize) * 4;
            dst[d..d + 4].copy_from_slice(&src.pixels[s..s + 4]);
        }
    }
    let mut out = src.clone();
    out.width = w;
    out.height = h;
    out.source = None; // o conteúdo foi reposicionado: vira raster
    out.pixels = std::sync::Arc::new(dst);
    if let Some(m) = out.mask.as_mut() {
        if m.data().len() == src.width as usize * src.height as usize {
            let mut md = vec![255u8; w as usize * h as usize];
            for y in 0..i64::from(src.height) {
                let ty = y + oy;
                if ty < 0 || ty >= i64::from(h) {
                    continue;
                }
                for x in 0..i64::from(src.width) {
                    let tx = x + ox;
                    if tx >= 0 && tx < i64::from(w) {
                        md[ty as usize * w as usize + tx as usize] =
                            m.data()[y as usize * src.width as usize + x as usize];
                    }
                }
            }
            m.set_data(std::sync::Arc::new(md));
        }
    }
    out
}

/// Desloca os pixels de uma camada em (dx, dy), descartando o que sai da área e preenchendo
/// com transparência.
pub fn shifted_pixels(layer: &Layer, dx: i32, dy: i32) -> Vec<u8> {
    let (w, h) = (layer.width as i64, layer.height as i64);
    let mut out = vec![0u8; layer.pixels.len()];
    for y in 0..h {
        let ty = y + i64::from(dy);
        if ty < 0 || ty >= h {
            continue;
        }
        let x0 = i64::from(dx).max(0);
        let x1 = (w + i64::from(dx)).min(w);
        if x0 >= x1 {
            continue;
        }
        let sx0 = x0 - i64::from(dx);
        let len = (x1 - x0) as usize * 4;
        let s = (y * w + sx0) as usize * 4;
        let d = (ty * w + x0) as usize * 4;
        out[d..d + len].copy_from_slice(&layer.pixels[s..s + len]);
    }
    out
}

/// IDs das camadas raster afetadas por mover `ids` (inclui descendentes de grupos).
pub fn movable_rasters(doc: &Document, ids: &[LayerId]) -> Vec<LayerId> {
    let mut all: HashSet<LayerId> = HashSet::new();
    for id in ids {
        all.insert(*id);
        all.extend(descendants(&doc.layers, *id));
    }
    doc.layers
        .iter()
        .filter(|l| all.contains(&l.id) && !l.is_group())
        .map(|l| l.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::BlendMode;
    use crate::edit::LayersEdit;
    use crate::History;

    fn doc3() -> (Document, LayerId, LayerId, LayerId) {
        let mut d = Document::new("t", 4, 4, Some([10, 10, 10, 255])).unwrap();
        let a = Layer::filled("A", 4, 4, [200, 0, 0, 255]);
        let b = Layer::filled("B", 4, 4, [0, 200, 0, 128]);
        let ids = (d.layers[0].id, a.id, b.id);
        d.layers.extend([a, b]);
        (d, ids.0, ids.1, ids.2)
    }

    fn names(d: &Document) -> Vec<String> {
        d.layers.iter().map(|l| l.name.clone()).collect()
    }

    fn apply(d: &mut Document, o: Outcome) {
        d.layers = o.layers;
    }

    #[test]
    fn duplicate_keeps_pixels_shared_and_uses_new_ids() {
        let (mut d, _, a, _) = doc3();
        let o = duplicate(&d, &[a]).unwrap();
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["Fundo", "A", "A cópia", "B"]);
        assert_ne!(d.layers[1].id, d.layers[2].id);
        assert!(std::sync::Arc::ptr_eq(
            &d.layers[1].pixels,
            &d.layers[2].pixels
        ));
    }

    #[test]
    fn group_ungroup_roundtrip_preserves_composite() {
        let (mut d, _, a, b) = doc3();
        let before = compose::flatten(&d);
        let o = group(&d, &[a, b], None);
        let gid = o.select[0];
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["Fundo", "Grupo", "A", "B"]);
        assert_eq!(d.layer(a).unwrap().parent, Some(gid));
        assert_eq!(compose::flatten(&d), before);
        let o = ungroup(&d, gid).unwrap();
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["Fundo", "A", "B"]);
        assert_eq!(d.layer(a).unwrap().parent, None);
        assert_eq!(compose::flatten(&d), before);
    }

    #[test]
    fn delete_group_removes_descendants_and_empty_group_is_possible() {
        let (mut d, _, a, b) = doc3();
        let o = group(&d, &[a, b], None);
        let gid = o.select[0];
        apply(&mut d, o);
        let o = delete(&d, &[gid]).unwrap();
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["Fundo"]);
        let o = group(&d, &[], Some(d.layers[0].id));
        apply(&mut d, o);
        assert!(d.layers[1].is_group());
        assert!(d.descendants(d.layers[1].id).is_empty());
    }

    #[test]
    fn move_up_down_swaps_siblings_only() {
        let (mut d, bg, a, b) = doc3();
        let o = move_sibling(&d, a, true).unwrap();
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["Fundo", "B", "A"]);
        assert!(move_sibling(&d, a, true).is_none());
        let o = move_sibling(&d, bg, true).unwrap();
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["B", "Fundo", "A"]);
        assert!(move_sibling(&d, b, false).is_none());
    }

    #[test]
    fn moving_a_group_moves_its_block() {
        let (mut d, bg, a, b) = doc3();
        let o = group(&d, &[a, b], None);
        let gid = o.select[0];
        apply(&mut d, o);
        let o = move_sibling(&d, gid, false).unwrap();
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["Grupo", "A", "B", "Fundo"]);
        assert_eq!(d.layer(bg).unwrap().parent, None);
    }

    #[test]
    fn merge_selected_equals_composite_of_those_layers() {
        let (mut d, _, a, b) = doc3();
        let expect = {
            let mut t = d.clone();
            t.layers.remove(0);
            compose::flatten(&t)
        };
        let o = merge_selected(&d, &[a, b]).unwrap();
        apply(&mut d, o);
        assert_eq!(names(&d), vec!["Fundo", "B"]);
        assert_eq!(d.layers[1].blend, BlendMode::Normal);
        assert_eq!(*d.layers[1].pixels, expect);
        assert!(merge_selected(&d, &[d.layers[1].id]).is_none());
    }

    #[test]
    fn merge_visible_keeps_hidden_layers() {
        let (mut d, _, a, b) = doc3();
        d.layer_mut(a).unwrap().visible = false;
        let o = merge_visible(&d).unwrap();
        apply(&mut d, o);
        assert_eq!(d.layers.len(), 2);
        assert!(d.layers.iter().any(|l| l.id == a && !l.visible));
        assert!(d.layer(b).is_none());
    }

    #[test]
    fn flatten_discards_hidden_and_matches_composite() {
        let (mut d, _, a, _) = doc3();
        d.layer_mut(a).unwrap().visible = false;
        let expect = compose::flatten(&d);
        let o = flatten_all(&d).unwrap();
        apply(&mut d, o);
        assert_eq!(d.layers.len(), 1);
        assert_eq!(*d.layers[0].pixels, expect);
        assert!(flatten_all(&d).is_none());
    }

    #[test]
    fn paste_across_sizes_centres_and_clips_and_assigns_new_ids() {
        let (d, _, a, _) = doc3();
        let clip = copy_nodes(&d, &[a]);
        let mut small = Document::new("s", 2, 2, Some([0, 0, 0, 255])).unwrap();
        let o = paste(&small, &clip, Some(small.layers[0].id)).unwrap();
        apply(&mut small, o);
        assert_eq!(small.layers.len(), 2);
        assert_ne!(small.layers[1].id, a);
        assert_eq!(small.layers[1].width, 2);
        assert_eq!(small.layers[1].pixel(0, 0).unwrap(), [200, 0, 0, 255]);
        // Destino maior: conteúdo centralizado, bordas transparentes.
        let mut big = Document::new("b", 8, 8, Some([0, 0, 0, 255])).unwrap();
        let o = paste(&big, &clip, None).unwrap();
        apply(&mut big, o);
        assert_eq!(big.layers[1].pixel(0, 0).unwrap()[3], 0);
        assert_eq!(big.layers[1].pixel(2, 2).unwrap(), [200, 0, 0, 255]);
    }

    #[test]
    fn structural_op_is_one_undo_step_and_shares_pixels() {
        let (mut d, _, a, b) = doc3();
        let mut h = History::new();
        let o = group(&d, &[a, b], None);
        let before = d.layers.clone();
        h.execute(
            &mut d,
            Box::new(LayersEdit::new("Agrupar", before.clone(), o.layers)),
        );
        assert_eq!(d.layers.len(), 4);
        h.undo(&mut d);
        assert_eq!(names(&d), vec!["Fundo", "A", "B"]);
        assert!(std::sync::Arc::ptr_eq(
            &d.layers[1].pixels,
            &before[1].pixels
        ));
        h.redo(&mut d);
        assert_eq!(d.layers.len(), 4);
    }

    #[test]
    fn normalize_repairs_orphans_and_cycles() {
        let mut g1 = Layer::group("g1");
        let mut g2 = Layer::group("g2");
        g1.parent = Some(g2.id);
        g2.parent = Some(g1.id);
        let mut orphan = Layer::transparent("o", 1, 1);
        orphan.parent = Some(LayerId(999_999));
        let out = normalize(vec![g1, g2, orphan]);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn shifted_pixels_clips_and_fills_transparent() {
        let mut l = Layer::transparent("l", 4, 3);
        l.set_pixel(0, 0, [1, 1, 1, 255]);
        l.set_pixel(3, 2, [2, 2, 2, 255]);
        let s = shifted_pixels(&l, 1, 1);
        let at = |x: usize, y: usize| s[(y * 4 + x) * 4 + 3];
        assert_eq!(at(1, 1), 255);
        assert_eq!(at(0, 0), 0);
        // (3,2) saiu da área e foi descartado.
        assert_eq!(s.iter().skip(3).step_by(4).filter(|a| **a > 0).count(), 1);
        let back = shifted_pixels(&l, -1, 0);
        assert_eq!(back[3], 0, "(0,0) saiu pela esquerda");
        let neg = shifted_pixels(&l, 0, 0);
        assert_eq!(neg, *l.pixels);
    }

    #[test]
    fn movable_rasters_includes_group_contents() {
        let (mut d, _, a, b) = doc3();
        let o = group(&d, &[a, b], None);
        let gid = o.select[0];
        apply(&mut d, o);
        let m = movable_rasters(&d, &[gid]);
        assert_eq!(m, vec![a, b]);
    }
}
