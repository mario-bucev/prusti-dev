// © 2019, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use encoder::vir;
use rustc::mir;
use std::collections::HashMap;
use std::fmt::{self, Debug, Display};
use std::iter::FromIterator;
use std::marker::Sized;

/// Backward interpreter for a loop-less MIR
pub trait BackwardMirInterpreter<'tcx> {
    type State: Sized;
    fn apply_terminator(
        &self,
        bb: mir::BasicBlock,
        terminator: &mir::Terminator<'tcx>,
        states: HashMap<mir::BasicBlock, &Self::State>,
    ) -> Self::State;
    fn apply_statement(
        &self,
        bb: mir::BasicBlock,
        stmt_index: usize,
        stmt: &mir::Statement<'tcx>,
        state: &mut Self::State,
    );
}

/// Interpret a loop-less MIR starting from the end and return the **initial** state.
/// The result is None if the CFG contains a loop.
pub fn run_backward_interpretation<'tcx, S: Debug, I: BackwardMirInterpreter<'tcx, State = S>>(
    mir: &mir::Mir<'tcx>,
    interpreter: &I,
) -> Option<S> {
    let basic_blocks = mir.basic_blocks();
    let mut heads: HashMap<mir::BasicBlock, S> = HashMap::new();
    let mut predecessors: HashMap<mir::BasicBlock, Vec<mir::BasicBlock>> = HashMap::new();

    // Compute the predecessors of each MIR block
    for bb in basic_blocks.indices() {
        predecessors.insert(bb, vec![]);
    }
    for (bb, bb_data) in basic_blocks.iter_enumerated() {
        if let Some(ref term) = bb_data.terminator {
            for succ_bb in term.successors() {
                let preds_of_succ = predecessors.get_mut(succ_bb).unwrap();
                preds_of_succ.push(bb);
            }
        }
    }

    // Find the final basic blocks
    let mut pending_blocks: Vec<mir::BasicBlock> = basic_blocks
        .iter_enumerated()
        .filter(|(_, bb_data)| match bb_data.terminator {
            Some(ref term) => term.successors().next().is_none(),
            _ => false,
        })
        .map(|(bb, _)| bb)
        .collect();

    // Interpret all the blocks in `pending_blocks`
    while !pending_blocks.is_empty() {
        let curr_bb = pending_blocks.pop().unwrap();
        let bb_data = &basic_blocks[curr_bb];

        // Apply the terminator
        let terminator = bb_data.terminator.as_ref().unwrap();
        let states = HashMap::from_iter(terminator.successors().map(|bb| (*bb, &heads[bb])));
        trace!("States before: {:?}", states);
        trace!("Apply terminator {:?}", terminator);
        let mut curr_state = interpreter.apply_terminator(curr_bb, terminator, states);
        trace!("State after: {:?}", curr_state);

        // Apply each statement, from the last
        for (stmt_index, stmt) in bb_data.statements.iter().enumerate().rev() {
            trace!("State before: {:?}", curr_state);
            trace!("Apply statement {:?}", stmt);
            interpreter.apply_statement(curr_bb, stmt_index, stmt, &mut curr_state);
            trace!("State after: {:?}", curr_state);
        }

        // Store the state at the beginning of block `curr_bb`
        heads.insert(curr_bb, curr_state);

        // Put the preceding basic blocks
        for pred_bb in &predecessors[&curr_bb] {
            if let Some(ref term) = basic_blocks[*pred_bb].terminator {
                if term.successors().all(|succ_bb| heads.contains_key(succ_bb)) {
                    pending_blocks.push(*pred_bb);
                }
            }
        }
    }

    let result = heads.remove(&basic_blocks.indices().next().unwrap());

    if result.is_none() {
        trace!("heads: {:?}", heads);
    }

    result
}

/// Interpret a loop-less MIR starting from the end and return the **initial** state.
/// The result is None if the CFG contains a loop.
pub fn run_backward_interpretation_point_to_point<
    'tcx,
    S: Debug + Clone,
    I: BackwardMirInterpreter<'tcx, State = S>,
>(
    mir: &mir::Mir<'tcx>,
    interpreter: &I,
    initial_bbi: mir::BasicBlock,
    final_bbi: mir::BasicBlock,
    final_stmt_index: usize,
    final_state: S,
    empty_state: S,
) -> Option<S> {
    let basic_blocks = mir.basic_blocks();
    let mut heads: HashMap<mir::BasicBlock, S> = HashMap::new();
    let mut predecessors: HashMap<mir::BasicBlock, Vec<mir::BasicBlock>> = HashMap::new();
    trace!(
        "[start] run_backward_interpretation_point_to_point:\n - from final block {:?}, statement {}\n - and state {:?}\n - to initial block {:?}\n - using empty state {:?}",
        final_bbi,
        final_stmt_index,
        final_state,
        initial_bbi,
        empty_state
    );

    // Compute the predecessors of each MIR block
    for bb in basic_blocks.indices() {
        predecessors.insert(bb, vec![]);
    }
    for (bb, bb_data) in basic_blocks.iter_enumerated() {
        if let Some(ref term) = bb_data.terminator {
            for succ_bb in term.successors() {
                let preds_of_succ = predecessors.get_mut(succ_bb).unwrap();
                preds_of_succ.push(bb);
            }
        }
    }

    // Find the final basic blocks
    let mut pending_blocks: Vec<mir::BasicBlock> = vec![final_bbi];

    // Interpret all the blocks in `pending_blocks`
    while !pending_blocks.is_empty() {
        let curr_bb = pending_blocks.pop().unwrap();
        let bb_data = &basic_blocks[curr_bb];
        trace!("curr_bb: {:?}", curr_bb);

        // Apply the terminator
        let terminator = bb_data.terminator.as_ref().unwrap();
        let terminator_index = bb_data.statements.len();
        let states = {
            // HACK: define the state even if only one successor is defined
            let default_state = terminator
                .successors()
                .flat_map(|bb| heads.get(bb))
                .next()
                .unwrap_or(&empty_state);
            HashMap::from_iter(
                terminator
                    .successors()
                    .map(|bb| (*bb, heads.get(bb).unwrap_or(&default_state))),
            )
        };
        trace!("States before: {:?}", states);
        trace!("Apply terminator {:?}", terminator);
        let mut curr_state = interpreter.apply_terminator(curr_bb, terminator, states);
        trace!("State after: {:?}", curr_state);
        if curr_bb == final_bbi && final_stmt_index == terminator_index {
            trace!("Final location reached in terminator");
            curr_state = final_state.clone();
            trace!("State after: {:?}", curr_state);
        }

        // Apply each statement, from the last
        for (stmt_index, stmt) in bb_data.statements.iter().enumerate().rev() {
            trace!("State before: {:?}", curr_state);
            trace!("Apply statement {:?}", stmt);
            interpreter.apply_statement(curr_bb, stmt_index, stmt, &mut curr_state);
            trace!("State after: {:?}", curr_state);
            if curr_bb == final_bbi && final_stmt_index == stmt_index {
                trace!("Final location reached in statement");
                curr_state = final_state.clone();
                trace!("State after: {:?}", curr_state);
            }
        }

        // Store the state at the beginning of block `curr_bb`
        heads.insert(curr_bb, curr_state);

        if curr_bb != initial_bbi {
            // Put the preceding basic blocks
            for pred_bb in &predecessors[&curr_bb] {
                // Note: here we don't check that all the successors of `pred_bb` has been visited.
                // It's a known limitation, because this is the point-to-point interpretation.
                // Use `run_backward_interpretation` if the check is important.
                pending_blocks.push(*pred_bb);
            }
        }
    }

    let result = heads.remove(&initial_bbi);

    if result.is_none() {
        trace!("heads: {:?}", heads);
    }

    trace!(
        "[end] run_backward_interpretation_point_to_point:\n - from final final block {:?}, statement {}\n - and state {:?}\n - to initial block {:?}\n - using empty state {:?}\n - resulted in state {:?}",
        final_bbi,
        final_stmt_index,
        final_state,
        initial_bbi,
        empty_state,
        result
    );

    result
}

/// Forward interpreter for a loop-less MIR
pub trait ForwardMirInterpreter<'tcx> {
    type State: Sized;
    fn initial_state(&self) -> Self::State;
    fn apply_statement(&self, stmt: &mir::Statement<'tcx>, state: &mut Self::State);
    fn apply_terminator(
        &self,
        terminator: &mir::Terminator<'tcx>,
        state: &Self::State,
    ) -> (HashMap<mir::BasicBlock, Self::State>, Option<Self::State>);
    fn join(&self, states: &[&Self::State]) -> Self::State;
}

#[derive(Clone, Debug)]
pub struct MultiExprBackwardInterpreterState {
    exprs: Vec<vir::Expr>,
}

impl Display for MultiExprBackwardInterpreterState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "exprs={}",
            self.exprs
                .iter()
                .map(|e| format!("{},", e))
                .collect::<String>()
        )
    }
}

impl MultiExprBackwardInterpreterState {
    pub fn new(exprs: Vec<vir::Expr>) -> Self {
        MultiExprBackwardInterpreterState { exprs }
    }

    pub fn new_single(expr: vir::Expr) -> Self {
        MultiExprBackwardInterpreterState { exprs: vec![expr] }
    }

    pub fn exprs(&self) -> &Vec<vir::Expr> {
        &self.exprs
    }

    pub fn into_expressions(self) -> Vec<vir::Expr> {
        self.exprs
    }

    pub fn substitute_place(&mut self, sub_target: &vir::Expr, replacement: vir::Expr) {
        trace!("substitute_place {:?} --> {:?}", sub_target, replacement);

        // If `replacement` is a reference, simplify also its dereferentiations
        if let vir::Expr::AddrOf(box ref base_replacement, ref _dereferenced_type, ref pos) =
            replacement
        {
            trace!("Substitution of a reference. Simplify its dereferentiations.");
            let deref_field = vir::Field::new("val_ref", base_replacement.get_type().clone());
            let deref_target = sub_target
                .clone()
                .field(deref_field.clone())
                .set_pos(pos.clone());
            self.substitute_place(&deref_target, base_replacement.clone());
        }

        for expr in &mut self.exprs {
            *expr = expr.clone().replace_place(&sub_target, &replacement);
        }
    }

    pub fn substitute_value(&mut self, exact_target: &vir::Expr, replacement: vir::Expr) {
        trace!("substitute_value {:?} --> {:?}", exact_target, replacement);
        for expr in &mut self.exprs {
            *expr = expr.clone().replace_place(exact_target, &replacement);
        }
    }

    pub fn use_place(&self, sub_target: &vir::Expr) -> bool {
        trace!("use_place {:?}", sub_target);
        self.exprs.iter().any(|expr| expr.find(sub_target))
    }
}
