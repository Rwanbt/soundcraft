//! Decoded picture buffer: reference marking (8.2.5), reference list construction (8.2.4) and output
//! ordering / bumping (C.4).

use crate::h264::error::{Result, ensure};
use crate::h264::picture::{FrameRef, RefPic};
use crate::h264::slice::{Mmco, SliceHeader, SliceType};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefMark {
    Unused,
    Short,
    Long,
}

/// Output-related metadata attached to a decoded frame.
#[derive(Clone, Debug)]
pub struct OutputMeta {
    pub pts: i64,
    pub key: bool,
    pub crop: (u32, u32, u32, u32),
    pub full_range: bool,
    pub colour_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
    pub sar: (u16, u16),
    /// Decoded without deblocking (draft mode, non-reference picture).
    pub draft: bool,
}

pub struct DpbEntry {
    pub frame: FrameRef,
    pub poc: i32,
    pub frame_num: u32,
    pub long_term_frame_idx: u32,
    pub mark: RefMark,
    pub needed_for_output: bool,
    pub non_existing: bool,
    pub meta: Arc<OutputMeta>,
}

pub struct Dpb {
    pub entries: Vec<DpbEntry>,
    /// None = "no long-term frame indices".
    pub max_long_term_frame_idx: Option<u32>,
    pub capacity: usize,
    pub max_reorder: usize,
}

/// A picture leaving the DPB for output.
pub struct Output {
    pub frame: FrameRef,
    pub poc: i32,
    pub meta: Arc<OutputMeta>,
}

impl Dpb {
    pub fn new() -> Self {
        Dpb { entries: Vec::new(), max_long_term_frame_idx: None, capacity: 16, max_reorder: 16 }
    }

    fn frame_num_wrap(e: &DpbEntry, cur_frame_num: u32, max_frame_num: u32) -> i32 {
        if e.frame_num > cur_frame_num { e.frame_num as i32 - max_frame_num as i32 } else { e.frame_num as i32 }
    }

    fn num_refs(&self) -> usize {
        self.entries.iter().filter(|e| e.mark != RefMark::Unused).count()
    }

    /// Remove entries not needed for output and not used for reference.
    fn prune(&mut self) {
        self.entries.retain(|e| e.needed_for_output || e.mark != RefMark::Unused);
    }

    /// Output the picture with the smallest POC (C.4.5.3 "bumping"). Returns false if none waits.
    fn bump(&mut self, out: &mut Vec<Output>) -> bool {
        let Some(i) = self.entries.iter().enumerate().filter(|(_, e)| e.needed_for_output).min_by_key(|(_, e)| e.poc).map(|(i, _)| i) else {
            return false;
        };
        let e = &mut self.entries[i];
        e.needed_for_output = false;
        out.push(Output { frame: e.frame.clone(), poc: e.poc, meta: e.meta.clone() });
        self.prune();
        true
    }

    /// Output everything in POC order and clear non-reference entries.
    pub fn flush(&mut self, out: &mut Vec<Output>) {
        while self.bump(out) {}
        self.prune();
    }

    /// IDR: mark all references unused; output (or drop) prior pictures.
    pub fn idr(&mut self, no_output_of_prior_pics: bool, out: &mut Vec<Output>) {
        for e in &mut self.entries {
            e.mark = RefMark::Unused;
        }
        if no_output_of_prior_pics {
            self.entries.clear();
        } else {
            self.flush(out);
        }
        self.max_long_term_frame_idx = None;
    }

    /// Sliding window marking (8.2.5.3).
    fn sliding_window(&mut self, cur_frame_num: u32, max_frame_num: u32, max_num_ref_frames: usize) {
        let limit = max_num_ref_frames.max(1);
        while self.num_refs() >= limit {
            let victim = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.mark == RefMark::Short)
                .min_by_key(|(_, e)| Self::frame_num_wrap(e, cur_frame_num, max_frame_num))
                .map(|(i, _)| i);
            match victim {
                Some(i) => self.entries[i].mark = RefMark::Unused,
                None => break,
            }
        }
        self.prune();
    }

    /// Insert "non-existing" frames for a frame_num gap (8.2.5.2).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_frame_num_gap(
        &mut self,
        prev_ref_frame_num: u32,
        frame_num: u32,
        max_frame_num: u32,
        max_num_ref_frames: usize,
        make_frame: &mut dyn FnMut(u32) -> (FrameRef, Arc<OutputMeta>),
        poc_state: &mut dyn FnMut(u32),
    ) {
        let mut unused = (prev_ref_frame_num + 1) % max_frame_num;
        let mut guard = 0;
        while unused != frame_num && guard < max_frame_num {
            self.sliding_window(unused, max_frame_num, max_num_ref_frames);
            let (frame, meta) = make_frame(unused);
            poc_state(unused);
            self.entries.push(DpbEntry {
                poc: frame.poc,
                frame,
                frame_num: unused,
                long_term_frame_idx: 0,
                mark: RefMark::Short,
                needed_for_output: false,
                non_existing: true,
                meta,
            });
            unused = (unused + 1) % max_frame_num;
            guard += 1;
        }
    }

    /// Initial reference picture lists (8.2.4.2) + modification (8.2.4.3) for a frame slice.
    pub fn build_ref_lists(&self, sh: &SliceHeader, cur_poc: i32, max_frame_num: u32) -> Result<[Vec<RefPic>; 2]> {
        let mut lists: [Vec<RefPic>; 2] = [Vec::new(), Vec::new()];
        if sh.slice_type.is_intra() {
            return Ok(lists);
        }
        let cur_fn = sh.frame_num;
        let pic_num = |e: &DpbEntry| Self::frame_num_wrap(e, cur_fn, max_frame_num);
        let short: Vec<&DpbEntry> = self.entries.iter().filter(|e| e.mark == RefMark::Short).collect();
        let mut long: Vec<&DpbEntry> = self.entries.iter().filter(|e| e.mark == RefMark::Long).collect();
        long.sort_by_key(|e| e.long_term_frame_idx);
        let to_ref = |e: &&DpbEntry| RefPic { frame: e.frame.clone(), long_term: e.mark == RefMark::Long };
        let mut entry_lists: [Vec<&DpbEntry>; 2] = [Vec::new(), Vec::new()];
        if sh.slice_type != SliceType::B {
            let mut s = short.clone();
            s.sort_by_key(|e| std::cmp::Reverse(pic_num(e)));
            entry_lists[0] = s.into_iter().chain(long.iter().copied()).collect();
        } else {
            let mut before: Vec<&DpbEntry> = short.iter().copied().filter(|e| e.poc < cur_poc).collect();
            let mut after: Vec<&DpbEntry> = short.iter().copied().filter(|e| e.poc > cur_poc).collect();
            before.sort_by_key(|e| std::cmp::Reverse(e.poc));
            after.sort_by_key(|e| e.poc);
            entry_lists[0] = before.iter().chain(after.iter()).chain(long.iter()).copied().collect();
            entry_lists[1] = after.iter().chain(before.iter()).chain(long.iter()).copied().collect();
            if entry_lists[1].len() > 1 && entry_lists[0].len() == entry_lists[1].len() {
                let same = entry_lists[0].iter().zip(entry_lists[1].iter()).all(|(a, b)| std::ptr::eq(*a, *b));
                if same {
                    entry_lists[1].swap(0, 1);
                }
            }
        }
        let nlists = if sh.slice_type == SliceType::B { 2 } else { 1 };
        for l in 0..nlists {
            let n = sh.num_ref_idx_active[l] as usize;
            let mut list: Vec<Option<&DpbEntry>> = entry_lists[l].iter().copied().map(Some).collect();
            list.truncate(n);
            list.resize(n, None);
            if !sh.ref_pic_list_mod[l].is_empty() {
                let max_pic_num = max_frame_num as i32;
                let curr_pic_num = cur_fn as i32;
                let mut pred = curr_pic_num;
                let mut ref_idx = 0usize;
                for m in &sh.ref_pic_list_mod[l] {
                    ensure!(ref_idx < n, "too many ref_pic_list_modification entries");
                    list.push(None); // temporarily one longer
                    let (target, is_long): (Option<&DpbEntry>, bool) = match m.idc {
                        0 | 1 => {
                            let d = m.value as i32 + 1;
                            let no_wrap = if m.idc == 0 {
                                if pred - d < 0 { pred - d + max_pic_num } else { pred - d }
                            } else if pred + d >= max_pic_num {
                                pred + d - max_pic_num
                            } else {
                                pred + d
                            };
                            pred = no_wrap;
                            let pn = if no_wrap > curr_pic_num { no_wrap - max_pic_num } else { no_wrap };
                            (short.iter().copied().find(|e| pic_num(e) == pn), false)
                        }
                        2 => (long.iter().copied().find(|e| e.long_term_frame_idx == m.value), true),
                        _ => continue,
                    };
                    let Some(target) = target else {
                        // Missing picture: keep the list unchanged (error concealment).
                        list.pop();
                        ref_idx += 1;
                        continue;
                    };
                    for c in (ref_idx + 1..=n).rev() {
                        list[c] = list[c - 1];
                    }
                    list[ref_idx] = Some(target);
                    ref_idx += 1;
                    let mut nidx = ref_idx;
                    for c in ref_idx..=n {
                        let keep = match list[c] {
                            None => true,
                            Some(e) => {
                                if is_long {
                                    !(e.mark == RefMark::Long && e.long_term_frame_idx == target.long_term_frame_idx)
                                } else {
                                    !(e.mark == RefMark::Short && std::ptr::eq(e, target))
                                }
                            }
                        };
                        if keep {
                            list[nidx] = list[c];
                            nidx += 1;
                        }
                    }
                    list.truncate(n);
                }
            }
            // Fill missing entries (should not happen in conforming streams) with the first valid entry.
            let fallback = list.iter().flatten().next().copied().or_else(|| self.entries.last());
            let out: Vec<RefPic> = list.iter().filter_map(|e| e.or(fallback)).map(|e| to_ref(&e)).collect();
            lists[l] = out;
        }
        Ok(lists)
    }

    /// Adaptive memory control (8.2.5.4). Returns true if MMCO 5 was executed.
    fn apply_mmcos(&mut self, mmcos: &[Mmco], cur_frame_num: u32, max_frame_num: u32, cur_long: &mut Option<u32>) -> bool {
        let mut had5 = false;
        let curr_pic_num = cur_frame_num as i32;
        for m in mmcos {
            match m.op {
                1 => {
                    let pn = curr_pic_num - (m.difference_of_pic_nums_minus1 as i32 + 1);
                    for e in &mut self.entries {
                        if e.mark == RefMark::Short && Self::frame_num_wrap(e, cur_frame_num, max_frame_num) == pn {
                            e.mark = RefMark::Unused;
                        }
                    }
                }
                2 => {
                    for e in &mut self.entries {
                        if e.mark == RefMark::Long && e.long_term_frame_idx == m.long_term_pic_num {
                            e.mark = RefMark::Unused;
                        }
                    }
                }
                3 => {
                    let pn = curr_pic_num - (m.difference_of_pic_nums_minus1 as i32 + 1);
                    for e in &mut self.entries {
                        if e.mark == RefMark::Long && e.long_term_frame_idx == m.long_term_frame_idx {
                            e.mark = RefMark::Unused;
                        }
                    }
                    for e in &mut self.entries {
                        if e.mark == RefMark::Short && Self::frame_num_wrap(e, cur_frame_num, max_frame_num) == pn {
                            e.mark = RefMark::Long;
                            e.long_term_frame_idx = m.long_term_frame_idx;
                        }
                    }
                }
                4 => {
                    let max = m.max_long_term_frame_idx_plus1;
                    for e in &mut self.entries {
                        if e.mark == RefMark::Long && (max == 0 || e.long_term_frame_idx > max - 1) {
                            e.mark = RefMark::Unused;
                        }
                    }
                    self.max_long_term_frame_idx = if max == 0 { None } else { Some(max - 1) };
                }
                5 => {
                    for e in &mut self.entries {
                        e.mark = RefMark::Unused;
                    }
                    self.max_long_term_frame_idx = None;
                    had5 = true;
                }
                6 => {
                    for e in &mut self.entries {
                        if e.mark == RefMark::Long && e.long_term_frame_idx == m.long_term_frame_idx {
                            e.mark = RefMark::Unused;
                        }
                    }
                    *cur_long = Some(m.long_term_frame_idx);
                }
                _ => {}
            }
        }
        self.prune();
        had5
    }

    /// Reference marking for the just-decoded picture and insertion into the DPB with output bumping.
    /// Returns pictures ready for output.
    #[allow(clippy::too_many_arguments)]
    pub fn store_picture(
        &mut self,
        sh: &SliceHeader,
        frame: FrameRef,
        poc: i32,
        max_frame_num: u32,
        max_num_ref_frames: usize,
        meta: Arc<OutputMeta>,
        out: &mut Vec<Output>,
    ) {
        let mut mark = RefMark::Unused;
        let mut long_idx = 0;
        let mut had5 = false;
        if sh.nal_ref_idc != 0 {
            if sh.idr {
                if sh.long_term_reference {
                    mark = RefMark::Long;
                    self.max_long_term_frame_idx = Some(0);
                } else {
                    mark = RefMark::Short;
                    self.max_long_term_frame_idx = None;
                }
            } else {
                let mut cur_long = None;
                if sh.adaptive_ref_pic_marking {
                    had5 = self.apply_mmcos(&sh.mmcos, sh.frame_num, max_frame_num, &mut cur_long);
                } else {
                    self.sliding_window(sh.frame_num, max_frame_num, max_num_ref_frames);
                }
                if let Some(i) = cur_long {
                    mark = RefMark::Long;
                    long_idx = i;
                } else {
                    mark = RefMark::Short;
                    // make room if MMCOs left the DPB over-full (non-conforming streams)
                    if self.num_refs() >= max_num_ref_frames.max(1) && sh.adaptive_ref_pic_marking {
                        self.sliding_window(sh.frame_num, max_frame_num, max_num_ref_frames);
                    }
                }
            }
        }
        if had5 {
            // All earlier pictures are output before the current one (C.4.4).
            self.flush(out);
        }
        let frame_num = if had5 { 0 } else { sh.frame_num };
        // Bumping until there is space (C.4.5.1 / C.4.5.2).
        if mark == RefMark::Unused {
            loop {
                let waiting = self.entries.len();
                if waiting < self.capacity {
                    break;
                }
                // non-reference picture with the smallest POC is output directly
                let min_poc = self.entries.iter().filter(|e| e.needed_for_output).map(|e| e.poc).min();
                if min_poc.is_none_or(|m| poc < m) {
                    out.push(Output { frame, poc, meta });
                    return;
                }
                if !self.bump(out) {
                    break;
                }
            }
        } else {
            while self.entries.len() >= self.capacity {
                if !self.bump(out) {
                    break;
                }
            }
        }
        self.entries.push(DpbEntry {
            frame,
            poc,
            frame_num,
            long_term_frame_idx: long_idx,
            mark,
            needed_for_output: true,
            non_existing: false,
            meta,
        });
        // Low-delay output: honour max_num_reorder_frames.
        while self.entries.iter().filter(|e| e.needed_for_output).count() > self.max_reorder {
            if !self.bump(out) {
                break;
            }
        }
    }
}

impl Default for Dpb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, poc: i32, frame_num: u32, mark: RefMark, output: bool) -> DpbEntry {
        DpbEntry {
            frame: Arc::new(crate::h264::picture::Frame::new(id, poc, 1, 1)),
            poc,
            frame_num,
            long_term_frame_idx: 0,
            mark,
            needed_for_output: output,
            non_existing: false,
            meta: Arc::new(OutputMeta {
                pts: i64::from(poc),
                key: false,
                crop: (0, 0, 16, 16),
                full_range: false,
                colour_primaries: 2,
                transfer_characteristics: 2,
                matrix_coefficients: 2,
                sar: (1, 1),
                draft: false,
            }),
        }
    }

    fn mmco(op: u32) -> Mmco {
        Mmco { op, difference_of_pic_nums_minus1: 0, long_term_pic_num: 0, long_term_frame_idx: 0, max_long_term_frame_idx_plus1: 0 }
    }

    #[test]
    fn mmco_1_and_2_unmark_only_the_targeted_reference_pictures() {
        let mut dpb = Dpb::new();
        dpb.entries = vec![entry(1, 1, 3, RefMark::Short, false), entry(2, 2, 2, RefMark::Short, false)];
        let mut long = entry(3, 3, 1, RefMark::Long, false);
        long.long_term_frame_idx = 7;
        dpb.entries.push(long);
        let mut cur_long = None;

        assert!(!dpb.apply_mmcos(&[mmco(1)], 4, 16, &mut cur_long));
        assert_eq!(dpb.entries.iter().map(|e| e.frame.id).collect::<Vec<_>>(), vec![2, 3]);
        let mut remove_long = mmco(2);
        remove_long.long_term_pic_num = 7;
        assert!(!dpb.apply_mmcos(&[remove_long], 4, 16, &mut cur_long));
        assert_eq!(dpb.entries.iter().map(|e| e.frame.id).collect::<Vec<_>>(), vec![2]);
        assert_eq!(cur_long, None);
    }

    #[test]
    fn mmco_3_replaces_long_term_slot_with_a_short_term_picture() {
        let mut dpb = Dpb::new();
        let mut displaced = entry(1, 10, 1, RefMark::Long, false);
        displaced.long_term_frame_idx = 2;
        dpb.entries.push(displaced);
        dpb.entries.push(entry(2, 20, 3, RefMark::Short, false));
        let mut convert = mmco(3);
        convert.long_term_frame_idx = 2;
        let mut cur_long = None;

        assert!(!dpb.apply_mmcos(&[convert], 4, 16, &mut cur_long));
        assert_eq!(dpb.entries.len(), 1);
        assert_eq!(dpb.entries[0].frame.id, 2);
        assert_eq!(dpb.entries[0].mark, RefMark::Long);
        assert_eq!(dpb.entries[0].long_term_frame_idx, 2);
    }

    #[test]
    fn mmco_4_5_and_6_enforce_long_term_limits_and_clear_prior_references() {
        let mut dpb = Dpb::new();
        for idx in 0..4 {
            let mut reference = entry(idx + 1, idx as i32, idx, RefMark::Long, false);
            reference.long_term_frame_idx = idx;
            dpb.entries.push(reference);
        }
        let mut cur_long = None;
        let mut limit = mmco(4);
        limit.max_long_term_frame_idx_plus1 = 2;
        assert!(!dpb.apply_mmcos(&[limit], 4, 16, &mut cur_long));
        assert_eq!(dpb.max_long_term_frame_idx, Some(1));
        assert_eq!(dpb.entries.iter().map(|e| e.long_term_frame_idx).collect::<Vec<_>>(), vec![0, 1]);

        let mut assign = mmco(6);
        assign.long_term_frame_idx = 1;
        assert!(!dpb.apply_mmcos(&[assign], 4, 16, &mut cur_long));
        assert_eq!(cur_long, Some(1));
        assert_eq!(dpb.num_refs(), 1);

        dpb.entries[0].needed_for_output = true;
        assert!(dpb.apply_mmcos(&[mmco(5)], 4, 16, &mut cur_long));
        assert_eq!(dpb.max_long_term_frame_idx, None);
        assert_eq!(dpb.num_refs(), 0);
        assert_eq!(dpb.entries.len(), 1);
        assert!(dpb.entries[0].needed_for_output);
    }

    #[test]
    fn flush_bumps_in_poc_order_and_preserves_references_without_reoutput() {
        let mut dpb = Dpb::new();
        dpb.entries = vec![
            entry(1, 20, 3, RefMark::Short, true),
            entry(2, -10, 0, RefMark::Unused, true),
            entry(3, 4, 2, RefMark::Long, true),
            entry(4, 1, 1, RefMark::Unused, false),
        ];
        let mut output = Vec::new();
        dpb.flush(&mut output);
        assert_eq!(output.iter().map(|o| o.poc).collect::<Vec<_>>(), vec![-10, 4, 20]);
        assert_eq!(output.iter().map(|o| o.frame.id).collect::<Vec<_>>(), vec![2, 3, 1]);
        assert_eq!(output.iter().map(|o| o.meta.pts).collect::<Vec<_>>(), vec![-10, 4, 20]);
        assert_eq!(dpb.entries.len(), 2);
        assert_eq!(dpb.num_refs(), 2);
        assert!(dpb.entries.iter().all(|e| !e.needed_for_output));
        dpb.flush(&mut output);
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn idr_can_flush_or_discard_waiting_pictures_without_leaking_references() {
        for discard in [false, true] {
            let mut dpb = Dpb::new();
            dpb.max_long_term_frame_idx = Some(7);
            dpb.entries = vec![entry(1, 12, 1, RefMark::Short, true), entry(2, -3, 2, RefMark::Long, true), entry(3, 5, 3, RefMark::Short, false)];
            let mut output = Vec::new();
            dpb.idr(discard, &mut output);
            let actual: Vec<i32> = output.iter().map(|o| o.poc).collect();
            assert_eq!(actual, if discard { vec![] } else { vec![-3, 12] });
            assert!(dpb.entries.is_empty());
            assert_eq!(dpb.num_refs(), 0);
            assert_eq!(dpb.max_long_term_frame_idx, None);
        }
    }

    #[test]
    fn sliding_window_evicts_oldest_wrapped_short_refs_but_retains_long_refs() {
        let mut dpb = Dpb::new();
        dpb.entries = vec![entry(1, 1, 14, RefMark::Short, true), entry(2, 2, 0, RefMark::Short, false), entry(3, 3, 1, RefMark::Long, false)];
        dpb.sliding_window(1, 16, 3);
        assert_eq!(dpb.num_refs(), 2);
        assert_eq!(dpb.entries.iter().find(|e| e.frame.id == 1).unwrap().mark, RefMark::Unused);
        assert_eq!(dpb.entries.iter().find(|e| e.frame.id == 2).unwrap().mark, RefMark::Short);
        dpb.sliding_window(1, 16, 2);
        assert_eq!(dpb.num_refs(), 1);
        assert!(dpb.entries.iter().all(|e| e.frame.id != 2));
        dpb.sliding_window(1, 16, 1);
        assert_eq!(dpb.num_refs(), 1);
        let mut output = Vec::new();
        dpb.flush(&mut output);
        assert_eq!(output.iter().map(|o| o.frame.id).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn frame_num_gap_synthesizes_wrapped_nonexisting_reference_frames() {
        let mut dpb = Dpb::new();
        let mut made = Vec::new();
        let mut advanced = Vec::new();
        dpb.fill_frame_num_gap(
            14,
            2,
            16,
            4,
            &mut |n| {
                made.push(n);
                let e = entry(n, n as i32, n, RefMark::Short, false);
                (e.frame, e.meta)
            },
            &mut |n| advanced.push(n),
        );
        assert_eq!(made, vec![15, 0, 1]);
        assert_eq!(advanced, made);
        assert_eq!(dpb.entries.iter().map(|e| e.frame_num).collect::<Vec<_>>(), made);
        assert_eq!(dpb.num_refs(), 3);
        assert!(dpb.entries.iter().all(|e| e.non_existing && !e.needed_for_output && e.mark == RefMark::Short));
        let mut output = Vec::new();
        dpb.flush(&mut output);
        assert!(output.is_empty());
        dpb.fill_frame_num_gap(1, 2, 16, 4, &mut |_| panic!("no gap to fill"), &mut |_| panic!("no gap to advance"));
        assert_eq!(dpb.num_refs(), 3);
    }

    #[test]
    fn empty_dpb_flush_is_idempotent_and_produces_no_output() {
        let mut dpb = Dpb::new();
        let mut output = Vec::new();
        assert!(dpb.entries.is_empty());
        assert_eq!(dpb.capacity, 16);
        assert_eq!(dpb.max_reorder, 16);
        assert_eq!(dpb.num_refs(), 0);
        assert!(!dpb.bump(&mut output));
        for _ in 0..3 {
            dpb.flush(&mut output);
            assert!(output.is_empty());
            assert!(dpb.entries.is_empty());
        }
    }

    #[test]
    fn idr_resets_long_term_reference_policy_for_both_output_modes() {
        for no_output in [false, true] {
            let mut dpb = Dpb::new();
            dpb.max_long_term_frame_idx = Some(15);
            let mut output = Vec::new();
            dpb.idr(no_output, &mut output);
            assert!(dpb.max_long_term_frame_idx.is_none());
            assert!(dpb.entries.is_empty());
            assert!(output.is_empty());
            dpb.idr(no_output, &mut output);
            assert!(output.is_empty());
        }
    }
}
