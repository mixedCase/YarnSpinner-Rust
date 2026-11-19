//! Adapted from <https://github.com/YarnSpinnerTool/YarnSpinner/blob/v3.3.1/YarnSpinner.Compiler/StringTableManager.cs>

use crate::output::StringInfo;
use crate::prelude::*;
use std::collections::HashMap;
use yarnspinner_core::prelude::*;

const MAX_ATTEMPTS: usize = 1000;

/// A line in the string table, plus what the compiler needs to report problems with it.
#[derive(Debug, Clone)]
pub(crate) struct StringTableEntry {
    pub(crate) info: StringInfo,
    /// An empty diagnostic pointing at the line ID tag, or the line if it has none.
    pub(crate) position: Diagnostic,
    pub(crate) has_expressions: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StringTableManager {
    entries: HashMap<LineId, StringTableEntry>,
    /// Line IDs in the order of registration, to keep diagnostics deterministic
    order: Vec<LineId>,
}

impl StringTableManager {
    pub(crate) fn contains_implicit_string_tags(&self) -> bool {
        self.entries
            .values()
            .any(|entry| entry.info.is_implicit_tag && entry.info.shadow_line_id.is_none())
    }

    pub(crate) fn get(&self, line_id: &LineId) -> Option<&StringTableEntry> {
        self.entries.get(line_id)
    }

    pub(crate) fn get_mut(&mut self, line_id: &LineId) -> Option<&mut StringTableEntry> {
        self.entries.get_mut(line_id)
    }

    /// The entries in the order they were registered.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&LineId, &StringTableEntry)> {
        self.order.iter().map(|id| (id, &self.entries[id]))
    }

    /// Registers a line. If `line_id` is `None`, a unique implicit ID is generated. Returns the ID of the line.
    pub(crate) fn insert(
        &mut self,
        line_id: Option<LineId>,
        string_info: StringInfo,
        position: Diagnostic,
        has_expressions: bool,
    ) -> LineId {
        let is_implicit_tag = line_id.is_none();
        let line_id = line_id.unwrap_or_else(|| {
            let candidate_seed = format!(
                "{}{}{}",
                string_info.file_name,
                string_info.node_name,
                self.entries.len()
            );
            // Implicit IDs of shadow lines are marked, so that they can be told apart from real lines
            let prefix = if string_info.shadow_line_id.is_some() { "sh_" } else { "" };
            (0..=MAX_ATTEMPTS)
                .find_map(|count| {
                    let suffix = if count == 0 { String::new() } else { count.to_string() };
                    let checksum = crc32_checksum_string(&format!("{candidate_seed}{suffix}"));
                    let id: LineId = format!("{LINE_ID_PREFIX}{prefix}{checksum}").into();
                    (!self.entries.contains_key(&id)).then_some(id)
                })
                .unwrap_or_else(|| panic!("Internal error: string table failed to find a non-colliding hash for \"{candidate_seed}\" after {MAX_ATTEMPTS} attempts"))
        });
        self.order.push(line_id.clone());
        self.entries.insert(
            line_id.clone(),
            StringTableEntry {
                info: StringInfo {
                    is_implicit_tag,
                    ..string_info
                },
                position,
                has_expressions,
            },
        );
        line_id
    }

    pub(crate) fn into_string_table(self) -> HashMap<LineId, StringInfo> {
        self.entries
            .into_iter()
            .map(|(id, entry)| (id, entry.info))
            .collect()
    }
}
