//! Histórico por documento com desfazer/refazer transacional.

use crate::document::Document;

/// Comando reversível aplicado a um documento.
pub trait Command {
    /// Rótulo exibido no painel de histórico.
    fn label(&self) -> &str;
    /// Aplica (ou reaplica) o comando.
    fn apply(&mut self, doc: &mut Document);
    /// Reverte o comando.
    fn revert(&mut self, doc: &mut Document);
    /// Memória aproximada retida pelo comando (para o orçamento do histórico).
    fn bytes(&self) -> usize {
        0
    }
}

/// Orçamento padrão de memória do histórico de um documento.
pub const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;

/// Pilha de desfazer/refazer. Cada comando é uma ação atômica do usuário.
#[derive(Default)]
pub struct History {
    done: Vec<Box<dyn Command>>,
    undone: Vec<Box<dyn Command>>,
    budget: Option<usize>,
}

impl History {
    pub fn new() -> Self {
        Self {
            budget: Some(DEFAULT_BUDGET),
            ..Self::default()
        }
    }

    /// Orçamento atual de memória (bytes), se houver.
    pub fn budget(&self) -> Option<usize> {
        self.budget
    }

    /// Altera o orçamento; ações antigas são descartadas se o novo limite for menor.
    pub fn set_budget(&mut self, budget: usize) {
        self.budget = Some(budget);
        self.enforce_budget();
    }

    /// Define o orçamento de memória; as ações mais antigas são descartadas ao excedê-lo.
    pub fn with_budget(budget: usize) -> Self {
        Self {
            budget: Some(budget),
            ..Self::default()
        }
    }

    fn enforce_budget(&mut self) {
        let Some(budget) = self.budget else { return };
        let total = |v: &Vec<Box<dyn Command>>| v.iter().map(|c| c.bytes()).sum::<usize>();
        while self.done.len() > 1 && total(&self.done) + total(&self.undone) > budget {
            self.done.remove(0);
        }
    }

    /// Executa um comando, registra-o e descarta a pilha de refazer.
    pub fn execute(&mut self, doc: &mut Document, mut cmd: Box<dyn Command>) {
        cmd.apply(doc);
        doc.touch();
        self.done.push(cmd);
        self.undone.clear();
        self.enforce_budget();
    }

    /// Registra um comando cuja alteração **já foi aplicada** (ex.: após arrastar um controle).
    pub fn record(&mut self, doc: &mut Document, cmd: Box<dyn Command>) {
        doc.touch();
        self.done.push(cmd);
        self.undone.clear();
        self.enforce_budget();
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Desfaz a última ação; retorna `false` se não havia nada a desfazer.
    pub fn undo(&mut self, doc: &mut Document) -> bool {
        match self.done.pop() {
            Some(mut cmd) => {
                cmd.revert(doc);
                doc.touch();
                self.undone.push(cmd);
                true
            }
            None => false,
        }
    }

    /// Refaz a última ação desfeita.
    pub fn redo(&mut self, doc: &mut Document) -> bool {
        match self.undone.pop() {
            Some(mut cmd) => {
                cmd.apply(doc);
                doc.touch();
                self.done.push(cmd);
                true
            }
            None => false,
        }
    }

    /// Rótulo da próxima ação a desfazer.
    pub fn undo_label(&self) -> Option<&str> {
        self.done.last().map(|c| c.label())
    }

    /// Rótulo da próxima ação a refazer.
    pub fn redo_label(&self) -> Option<&str> {
        self.undone.last().map(|c| c.label())
    }

    /// Rótulos das ações realizadas, da mais antiga à mais recente.
    pub fn labels(&self) -> Vec<&str> {
        self.done.iter().map(|c| c.label()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SetPixel {
        pos: (u32, u32),
        new: [u8; 4],
        old: [u8; 4],
    }

    impl Command for SetPixel {
        fn label(&self) -> &str {
            "Pixel"
        }
        fn apply(&mut self, doc: &mut Document) {
            let l = &mut doc.layers[0];
            self.old = l.pixel(self.pos.0, self.pos.1).unwrap();
            l.set_pixel(self.pos.0, self.pos.1, self.new);
        }
        fn revert(&mut self, doc: &mut Document) {
            doc.layers[0].set_pixel(self.pos.0, self.pos.1, self.old);
        }
    }

    fn set(x: u32, rgba: [u8; 4]) -> Box<SetPixel> {
        Box::new(SetPixel {
            pos: (x, 0),
            new: rgba,
            old: [0; 4],
        })
    }

    #[test]
    fn undo_redo_roundtrip_and_revision() {
        let mut doc = Document::new("t", 4, 4, None).unwrap();
        let mut h = History::new();
        h.execute(&mut doc, set(1, [9, 8, 7, 255]));
        assert_eq!(doc.layers[0].pixel(1, 0), Some([9, 8, 7, 255]));
        assert!(doc.modified);
        assert!(h.undo(&mut doc));
        assert_eq!(doc.layers[0].pixel(1, 0), Some([0, 0, 0, 0]));
        assert!(h.redo(&mut doc));
        assert_eq!(doc.layers[0].pixel(1, 0), Some([9, 8, 7, 255]));
        assert_eq!(doc.revision, 3);
    }

    #[test]
    fn new_action_clears_redo() {
        let mut doc = Document::new("t", 2, 2, None).unwrap();
        let mut h = History::new();
        h.execute(&mut doc, set(0, [1, 1, 1, 255]));
        h.undo(&mut doc);
        h.execute(&mut doc, set(1, [1, 1, 1, 255]));
        assert!(!h.can_redo());
        assert_eq!(h.labels().len(), 1);
    }

    #[test]
    fn budget_drops_oldest_actions() {
        struct Big;
        impl Command for Big {
            fn label(&self) -> &str {
                "Big"
            }
            fn apply(&mut self, _: &mut Document) {}
            fn revert(&mut self, _: &mut Document) {}
            fn bytes(&self) -> usize {
                100
            }
        }
        let mut doc = Document::new("t", 2, 2, None).unwrap();
        let mut h = History::with_budget(250);
        for _ in 0..5 {
            h.execute(&mut doc, Box::new(Big));
        }
        assert_eq!(h.labels().len(), 2);
    }

    #[test]
    fn rejects_invalid_dimensions() {
        assert!(Document::new("x", 0, 10, None).is_err());
        assert!(Document::new("x", 40_000, 10, None).is_err());
    }
}
