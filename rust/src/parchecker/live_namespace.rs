//! Bounded source namespace identity, independent of source I/O and GF work.
use super::{live_error, live_hint_hash16k, FileId, PacketSet, RepairError};
use crate::parlive::ControlLease;
use std::cmp::Ordering;
use std::mem::size_of;
use std::path::PathBuf;

const MAX_ROWS: usize = 32768;
const SCRATCH_BYTES: usize = 4096;

pub(crate) struct NamespaceRow {
    pub queue_id: i32,
    pub name: Vec<u8>,
    pub original: Vec<u8>,
    pub hash16k: Vec<u8>,
    pub output: Vec<u8>,
    pub par_file: bool,
    pub deleted: bool,
    pub completed: bool,
}

/// The native producer must reserve before copying these facts. The established
/// root's physical identity is a caller precondition, not proved by its spelling.
pub(crate) struct NamespaceFacts {
    pub root: PathBuf,
    pub rows: Vec<NamespaceRow>,
    // Last: retained strings and rows must die before their charge.
    pub reservation: ControlLease,
}

struct CatalogEntry {
    id: FileId,
    name: Vec<u8>,
    hash: [u8; 16],
}

pub(crate) struct NamespaceProof {
    catalog: Vec<CatalogEntry>,
    hash_order: Vec<usize>,
    bindings: Vec<Option<usize>>,
    reverse_owner: Vec<Option<i32>>,
    seen: Vec<u8>,
    invalid: bool,
    // Last: the lease also owns every index and catalog allocation above.
    facts: NamespaceFacts,
}

fn invalid(message: &'static str) -> RepairError {
    live_error("live source namespace", message)
}

fn reserved<T>(count: usize) -> Result<Vec<T>, RepairError> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(count)
        .map_err(|_| invalid("metadata allocation refused"))?;
    Ok(rows)
}

fn alias_cmp(left: &[u8], right: &[u8]) -> Ordering {
    left.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(right.iter().map(u8::to_ascii_lowercase))
}

fn eligible_leaf(name: &[u8]) -> bool {
    if name.is_empty()
        || name.len() > 255
        || name[0] == b' '
        || matches!(name.last(), Some(b'.' | b' '))
        || !name.iter().all(|&byte| {
            (b' '..=b'~').contains(&byte)
                && !matches!(
                    byte,
                    b'/' | b'\\' | b':' | b'"' | b'<' | b'>' | b'|' | b'?' | b'*'
                )
        })
        || name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(b".tmp")
    {
        return false;
    }
    let stem = name.split(|&b| b == b'.').next().unwrap();
    if [b"con".as_slice(), b"prn", b"aux", b"nul"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
        || stem.len() == 4
            && (stem[..3].eq_ignore_ascii_case(b"com") || stem[..3].eq_ignore_ascii_case(b"lpt"))
            && (b'1'..=b'9').contains(&stem[3])
    {
        return false;
    }
    // The temporary article directory can coincide with this source root.
    if let Some(dot) = name.iter().position(|&b| b == b'.') {
        if dot > 0
            && name[..dot].iter().all(u8::is_ascii_digit)
            && name[dot + 1..].len() >= 3
            && name[dot + 1..].iter().all(u8::is_ascii_digit)
        {
            return false;
        }
    }
    true
}

fn fact_bytes(facts: &NamespaceFacts) -> Option<usize> {
    let initial = facts
        .rows
        .capacity()
        .checked_mul(size_of::<NamespaceRow>())?
        .checked_add(facts.root.capacity())?;
    facts.rows.iter().try_fold(initial, |sum, row| {
        [
            row.name.capacity(),
            row.original.capacity(),
            row.hash16k.capacity(),
            row.output.capacity(),
        ]
        .iter()
        .try_fold(sum, |sum, bytes| sum.checked_add(*bytes))
    })
}

fn output_belongs(root: &std::path::Path, row: &NamespaceRow) -> bool {
    output_matches(root, row.queue_id, &row.name, &row.output)
}

fn output_matches(root: &std::path::Path, id: i32, name: &[u8], bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let output = crate::fsops::path(bytes);
    if output.parent() != Some(root) {
        return false;
    }
    let Some(leaf) = output.file_name().map(|s| s.as_encoded_bytes()) else {
        return false;
    };
    if leaf == name {
        return true;
    }
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    let mut value = id as u32;
    loop {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    leaf.strip_suffix(b".out.tmp") == Some(&digits[at..])
}

impl NamespaceProof {
    pub(crate) fn protects_leaf(&self, name: &[u8]) -> bool {
        self.catalog.iter().any(|entry| entry.name.eq_ignore_ascii_case(name))
            || self.facts.rows.iter().any(|row| row.name.eq_ignore_ascii_case(name))
    }
    /// Compute outside the queue, before any native metadata is copied.
    pub fn catalog_required_bytes(set: &PacketSet) -> Result<usize, RepairError> {
        let main = set.main.as_ref().ok_or(RepairError::NoMainPacket)?;
        let count = main.recoverable_file_ids.len();
        if count == 0 || count > MAX_ROWS || main.recoverable_count as usize != count {
            return Err(invalid("unprovable catalog geometry"));
        }
        let per = size_of::<CatalogEntry>() + size_of::<usize>() + size_of::<Option<i32>>();
        let bytes = count
            .checked_mul(per)
            .and_then(|n| n.checked_add(size_of::<Self>() + SCRATCH_BYTES))
            .and_then(|n| {
                main.recoverable_file_ids.iter().try_fold(n, |sum, id| {
                    sum.checked_add(set.file_descs.get(id)?.name.len())
                })
            });
        bytes.ok_or_else(|| invalid("catalog sizing overflow or missing description"))
    }

    pub fn prepare(set: &PacketSet, mut facts: NamespaceFacts) -> Result<Self, RepairError> {
        let main = set.main.as_ref().ok_or(RepairError::NoMainPacket)?;
        let count = main.recoverable_file_ids.len();
        if count == 0
            || count > MAX_ROWS
            || main.recoverable_count as usize != count
            || facts.rows.len() > MAX_ROWS
            || !facts.root.is_absolute()
        {
            return Err(invalid("unprovable root or metadata geometry"));
        }
        let catalog_bytes = Self::catalog_required_bytes(set)?;
        let per_row = size_of::<Option<usize>>() + size_of::<usize>() + size_of::<u8>();
        let required = fact_bytes(&facts)
            .and_then(|sum| sum.checked_add(catalog_bytes))
            .and_then(|sum| sum.checked_add(facts.rows.len().checked_mul(per_row)?));
        if required.is_none_or(|bytes| bytes as u64 > facts.reservation.bytes()) {
            return Err(invalid("metadata exceeds retained control reservation"));
        }
        facts.rows.sort_unstable_by_key(|row| row.queue_id);
        for row in &facts.rows {
            if row.queue_id <= 0 || row.deleted || !eligible_leaf(&row.name) {
                return Err(invalid("ambiguous queue identity"));
            }
            if !output_belongs(&facts.root, row) {
                return Err(invalid("physical output does not belong to its queue row"));
            }
        }
        for group in facts.rows.chunk_by(|a, b| a.queue_id == b.queue_id) {
            if group.len() > 2 || group.len() == 2 && group[0].completed == group[1].completed {
                return Err(invalid("duplicate queue/completed role"));
            }
        }
        let mut conflicting = false;
        facts.rows.dedup_by(|current, previous| {
            if current.queue_id != previous.queue_id {
                return false;
            }
            if current.completed == previous.completed
                || current.name != previous.name
                || current.par_file != previous.par_file
                || !current.hash16k.is_empty()
                    && !previous.hash16k.is_empty()
                    && current.hash16k != previous.hash16k
                || !current.original.is_empty()
                    && !previous.original.is_empty()
                    && current.original != previous.original
            {
                conflicting = true;
            }
            if previous.hash16k.is_empty() {
                std::mem::swap(&mut previous.hash16k, &mut current.hash16k);
            }
            previous.completed = true;
            true
        });
        if conflicting {
            return Err(invalid("inconsistent queue/completed handoff"));
        }
        let mut catalog = reserved(count)?;
        for id in &main.recoverable_file_ids {
            let desc = set
                .file_descs
                .get(id)
                .ok_or_else(|| invalid("missing file description"))?;
            if desc.file_id != *id || !desc.id_consistent() || !eligible_leaf(&desc.name) {
                return Err(invalid("unprovable PAR destination"));
            }
            // Eligible ASCII names have no sanitisation or decoding ambiguity.
            if desc.local_name().as_bytes() != desc.name {
                return Err(invalid("PAR destination changes on local translation"));
            }
            let mut name = reserved(desc.name.len())?;
            name.extend_from_slice(&desc.name);
            catalog.push(CatalogEntry {
                id: *id,
                name,
                hash: desc.hash_16k,
            });
        }
        catalog.sort_unstable_by(|a, b| alias_cmp(&a.name, &b.name));
        if catalog
            .windows(2)
            .any(|pair| alias_cmp(&pair[0].name, &pair[1].name).is_eq())
        {
            return Err(invalid("PAR destinations have potential aliases"));
        }
        let mut hash_order = reserved(count)?;
        hash_order.extend(0..count);
        hash_order.sort_unstable_by_key(|&index| catalog[index].hash);
        let mut bindings = reserved(facts.rows.len())?;
        bindings.resize(facts.rows.len(), None);
        let mut reverse_owner = reserved(count)?;
        reverse_owner.resize(count, None);
        let mut seen = reserved(facts.rows.len())?;
        seen.resize(facts.rows.len(), 0);
        let mut proof = Self {
            catalog,
            hash_order,
            bindings,
            reverse_owner,
            seen,
            invalid: false,
            facts,
        };
        // An index only; no second copy of queue strings and no stable-sort scratch.
        let mut names = reserved(proof.facts.rows.len())?;
        names.extend(0..proof.facts.rows.len());
        names.sort_unstable_by(|&a, &b| {
            alias_cmp(&proof.facts.rows[a].name, &proof.facts.rows[b].name)
        });
        if names.windows(2).any(|pair| {
            alias_cmp(
                &proof.facts.rows[pair[0]].name,
                &proof.facts.rows[pair[1]].name,
            )
            .is_eq()
        }) {
            return Err(invalid("queue paths have potential aliases"));
        }
        let actual = proof.allocated_bytes(names.capacity());
        if actual.is_none_or(|bytes| bytes as u64 > proof.facts.reservation.bytes()) {
            return Err(invalid("allocated metadata exceeds control reservation"));
        }
        for index in 0..proof.facts.rows.len() {
            let row = &proof.facts.rows[index];
            let candidate = proof.candidate(&row.name, &row.hash16k)?;
            if row.par_file && candidate.is_some() {
                return Err(invalid("recovery entry aliases a source destination"));
            }
            proof.assign(index, candidate)?;
        }
        Ok(proof)
    }

    /// The direct renamer gave a queue row its real name (obfuscated posts).
    /// Follow it when the new name is unambiguous: an eligible leaf that no
    /// other row aliases; for a source, either the one PAR destination (by
    /// name, agreeing with any known 16k hash) it is or may become bound to,
    /// or, while unbound, a name outside this set; for a recovery row, never a
    /// source destination. Anything else still invalidates the namespace.
    /// Longer names grow the control reservation on demand.
    fn follow_rename(&mut self, row: usize, name: &[u8], hint: &[u8]) -> Result<(), RepairError> {
        if !eligible_leaf(name) {
            return Err(invalid("renamed queue identity is not an eligible leaf"));
        }
        if self
            .facts
            .rows
            .iter()
            .enumerate()
            .any(|(i, other)| i != row && alias_cmp(&other.name, name).is_eq())
        {
            return Err(invalid("renamed queue path aliases another row"));
        }
        // By name only: a source must take exactly its PAR destination's name,
        // the path live PAR reads and repairs.
        let candidate = self.candidate(name, b"")?;
        if self.facts.rows[row].par_file {
            if candidate.is_some() {
                return Err(invalid("recovery entry aliases a source destination"));
            }
        } else if let Some(index) = candidate {
            let id = self.facts.rows[row].queue_id;
            if self.bindings[row].is_some_and(|old| old != index)
                || self.reverse_owner[index].is_some_and(|owner| owner != id)
            {
                return Err(invalid("PAR source has a different queue owner"));
            }
            // The direct renamer picks the first PAR entry with this row's 16k
            // hash, so a hash that matches this destination must match only
            // it. A hash naming a different entry contradicts the name. A hash
            // matching no entry did not drive the rename (the name came from
            // the article itself; the hashed first bytes were damaged or not
            // the file's start), so the name stands and slice CRCs verify it.
            let known = if hint.is_empty() { &self.facts.rows[row].hash16k[..] } else { hint };
            if let Some(hash) = live_hint_hash16k(known) {
                let matches = self
                    .hash_order
                    .iter()
                    .filter(|&&i| self.catalog[i].hash == hash)
                    .count();
                let ours = self.catalog[index].hash == hash;
                if (ours && matches > 1) || (!ours && matches > 0) {
                    return Err(invalid("renamed source hash does not prove its destination"));
                }
            }
        } else if self.bindings[row].is_some() {
            return Err(invalid("bound source renamed away from its PAR destination"));
        }
        // An unbound source may take a name outside this PAR set (another set
        // in the same job); it stays unbound.
        let mut renamed = reserved(name.len())?;
        renamed.extend_from_slice(name);
        let previous = std::mem::replace(&mut self.facts.rows[row].name, renamed);
        let fits = self.allocated_bytes(0).is_some_and(|bytes| {
            let held = self.facts.reservation.bytes();
            bytes as u64 <= held || self.facts.reservation.grow(bytes as u64 - held).is_ok()
        });
        if !fits {
            self.facts.rows[row].name = previous;
            return Err(invalid("renamed metadata exceeds control reservation"));
        }
        Ok(())
    }

    fn allocated_bytes(&self, scratch_capacity: usize) -> Option<usize> {
        let mut sum = fact_bytes(&self.facts)?.checked_add(size_of::<Self>() + SCRATCH_BYTES)?;
        for (count, size) in [
            (self.catalog.capacity(), size_of::<CatalogEntry>()),
            (self.hash_order.capacity(), size_of::<usize>()),
            (self.bindings.capacity(), size_of::<Option<usize>>()),
            (self.reverse_owner.capacity(), size_of::<Option<i32>>()),
            (self.seen.capacity(), size_of::<u8>()),
            (scratch_capacity, size_of::<usize>()),
        ] {
            sum = sum.checked_add(count.checked_mul(size)?)?;
        }
        self.catalog
            .iter()
            .try_fold(sum, |sum, entry| sum.checked_add(entry.name.capacity()))
    }

    fn candidate(&self, name: &[u8], hint: &[u8]) -> Result<Option<usize>, RepairError> {
        if let Ok(index) = self
            .catalog
            .binary_search_by(|entry| alias_cmp(&entry.name, name))
        {
            if self.catalog[index].name != name {
                return Err(invalid(
                    "queue spelling aliases a different PAR destination",
                ));
            }
            return Ok(Some(index));
        }
        if hint.is_empty() {
            return Ok(None);
        }
        let hash = live_hint_hash16k(hint).ok_or_else(|| invalid("malformed hash hint"))?;
        let Ok(at) = self
            .hash_order
            .binary_search_by_key(&hash, |&i| self.catalog[i].hash)
        else {
            return Ok(None);
        };
        if at > 0 && self.catalog[self.hash_order[at - 1]].hash == hash
            || at + 1 < self.hash_order.len() && self.catalog[self.hash_order[at + 1]].hash == hash
        {
            return Err(invalid("ambiguous hash-only source mapping"));
        }
        Ok(Some(self.hash_order[at]))
    }

    fn assign(
        &mut self,
        row: usize,
        candidate: Option<usize>,
    ) -> Result<Option<FileId>, RepairError> {
        let Some(index) = candidate.or(self.bindings[row]) else {
            return Ok(None);
        };
        let id = self.facts.rows[row].queue_id;
        if self.bindings[row].is_some_and(|old| old != index)
            || self.reverse_owner[index].is_some_and(|owner| owner != id)
        {
            return Err(invalid("PAR source has a different queue owner"));
        }
        self.reverse_owner[index] = Some(id);
        self.bindings[row] = Some(index);
        Ok(Some(self.catalog[index].id))
    }

    pub fn bind(
        &mut self,
        queue_id: i32,
        name: &[u8],
        hint: &[u8],
    ) -> Result<Option<FileId>, RepairError> {
        let result = (|| {
            if self.invalid {
                return Err(invalid("namespace was invalidated"));
            }
            let row = self
                .facts
                .rows
                .binary_search_by_key(&queue_id, |row| row.queue_id)
                .map_err(|_| invalid("writer is absent from the captured namespace"))?;
            if self.facts.rows[row].par_file {
                return Err(invalid("writer identity differs from its captured row"));
            }
            if self.facts.rows[row].name != name {
                self.follow_rename(row, name, hint)?;
            }
            let candidate = self.candidate(name, hint)?;
            if let Some(bound) = self.bindings[row] {
                if !hint.is_empty()
                    && candidate.is_none()
                    && live_hint_hash16k(hint) != Some(self.catalog[bound].hash)
                {
                    return Err(invalid("bound source hash identity changed"));
                }
            }
            self.assign(row, candidate)
        })();
        if result.is_err() {
            self.invalid = true;
        }
        result
    }

    pub fn begin_recheck(&mut self) {
        self.seen.fill(0);
    }

    /// Identity check of one currently borrowed native row. Allocates only
    /// to follow a direct rename.
    pub fn recheck_row(
        &mut self,
        queue_id: i32,
        flags: u32,
        fields: [&[u8]; 4],
    ) -> Result<(), RepairError> {
        let result = (|| {
            if self.invalid || flags & !7 != 0 || flags & 2 != 0 {
                return Err(invalid("invalid current row"));
            }
            let row = self
                .facts
                .rows
                .binary_search_by_key(&queue_id, |row| row.queue_id)
                .map_err(|_| invalid("new queue owner appeared"))?;
            if self.facts.rows[row].name != fields[0]
                && self.facts.rows[row].par_file == (flags & 1 != 0)
            {
                self.follow_rename(row, fields[0], fields[2])?;
            }
            let old = &self.facts.rows[row];
            if old.name != fields[0]
                || old.par_file != (flags & 1 != 0)
                || !output_matches(&self.facts.root, queue_id, fields[0], fields[3])
            {
                return Err(invalid("namespace ownership changed"));
            }
            let bit = if flags & 4 == 0 { 1 } else { 2 };
            if self.seen[row] & bit != 0 {
                return Err(invalid("duplicate current owner"));
            }
            self.seen[row] |= bit;
            if !old.par_file {
                self.bind(queue_id, fields[0], fields[2])?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.invalid = true;
        }
        result
    }

    pub fn finish_recheck(&mut self) -> Result<(), RepairError> {
        if self.invalid
            || self
                .seen
                .iter()
                .enumerate()
                .any(|(i, seen)| *seen == 0 || self.facts.rows[i].completed && *seen & 2 == 0)
        {
            self.invalid = true;
            return Err(invalid("captured owner disappeared or regressed"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
