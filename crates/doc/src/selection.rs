//! Selection state (objects and, for direct selection, individual anchors).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Document, NodeId};

/// (subpath index, anchor index) inside a path.
pub type AnchorRef = (usize, usize);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    /// Selected objects in selection order.
    pub objects: Vec<NodeId>,
    /// Direct-selected anchors per path. A path in `objects` with no entry here is fully selected.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub anchors: BTreeMap<NodeId, BTreeSet<AnchorRef>>,
    /// Key object for Align.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<NodeId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
    pub fn len(&self) -> usize {
        self.objects.len()
    }
    pub fn contains(&self, id: NodeId) -> bool {
        self.objects.contains(&id)
    }
    pub fn clear(&mut self) {
        self.objects.clear();
        self.anchors.clear();
        self.key = None;
    }
    pub fn set(&mut self, ids: impl IntoIterator<Item = NodeId>) {
        self.clear();
        for id in ids {
            self.add(id);
        }
    }
    pub fn add(&mut self, id: NodeId) {
        if !self.objects.contains(&id) {
            self.objects.push(id);
        }
    }
    pub fn remove(&mut self, id: NodeId) {
        self.objects.retain(|x| *x != id);
        self.anchors.remove(&id);
        if self.key == Some(id) {
            self.key = None;
        }
    }
    pub fn toggle(&mut self, id: NodeId) {
        if self.contains(id) { self.remove(id) } else { self.add(id) }
    }
    /// Are some (not all) anchors of `id` direct-selected?
    pub fn partial(&self, id: NodeId) -> Option<&BTreeSet<AnchorRef>> {
        self.anchors.get(&id)
    }
    /// Drop ids that no longer exist or are no longer editable.
    pub fn prune(&mut self, doc: &Document) {
        self.objects.retain(|id| doc.node(*id).is_some());
        self.anchors.retain(|id, _| doc.node(*id).is_some());
        if self.key.is_some_and(|k| doc.node(k).is_none()) {
            self.key = None;
        }
    }
    /// Top-level ordering: selected ids sorted by paint order (bottom first).
    pub fn in_paint_order(&self, doc: &Document) -> Vec<NodeId> {
        let mut v: Vec<(Vec<usize>, NodeId)> = self.objects.iter().filter_map(|id| doc.index_path(*id).map(|p| (p, *id))).collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ops() {
        let mut s = Selection::default();
        s.add(NodeId(1));
        s.add(NodeId(1));
        s.toggle(NodeId(2));
        assert_eq!(s.objects, vec![NodeId(1), NodeId(2)]);
        s.toggle(NodeId(1));
        assert_eq!(s.objects, vec![NodeId(2)]);
        s.clear();
        assert!(s.is_empty());
    }
}
