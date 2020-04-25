// © 2019, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use encoder::foldunfold::perm::*;
use encoder::vir;
use encoder::vir::{PermAmount, FoldingBehaviour, Position};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Fold(String, Vec<vir::Expr>, PermAmount, vir::MaybeEnumVariantIndex, vir::Position),
    Unfold(String, Vec<vir::Expr>, PermAmount, vir::MaybeEnumVariantIndex),
    /// The dropped perm and the missing permission that caused this
    /// perm to be dropped.
    Drop(Perm, Perm),
    Assertion(vir::Expr),
    /// An unfold that must be directly folded back once the statement
    /// that needs it is finished.
    /// This is necessary when dealing with quantified predicate accesses.
    /// For instance, suppose that we have the following quantified predicate:
    /// ```forall i: Int :: { self.val_array[i] } 0 <= i && i < |self.val_array|
    ///     ==> acc(isize(self.val_array[i].val_ref))```
    /// Furthermore, assume that we have the following statement
    /// ```foo.val_array[idx].val_ref.val_int = 42```
    /// We need to unfold ```isize(foo.val_array[idx].val_ref)``` before we can
    /// do the assignment.
    /// However, once the assignment is done, we must fold it back.
    /// Indeed, if we don't do so, we can have the following statement
    /// ```foo.val_array[idx2].val_ref.val_int = 34```, which would then cause
    /// the unfolding of ```isize(self.val_array[idx2].val_ref)```
    /// If both ```isize(self.val_array[idx].val_ref)``` and
    /// ```isize(self.val_array[idx2].val_ref)``` are unfolded with `write`,
    /// the prover may deduce that `idx == idx2`, which may not be the case!
    ///
    /// Note that this problem shouldn't arise for `read` accesses, but we still
    /// conservatively temporarily unfold `read` (instantiated) predicate accesses.
    TemporaryUnfold(String, Vec<vir::Expr>, PermAmount, vir::MaybeEnumVariantIndex),
    /// An `unfolding` expression happening inside a `forall` expression.
    /// For instance, suppose that we have the following quantified predicate:
    /// ```forall i: Int :: { foo.val_array[i] } 0 <= i && i < |foo.val_array|
    ///     ==> acc(isize(foo.val_array[i].val_ref))```
    /// And that we have the following expression:
    /// ```forall i: Int :: { foo.val_array[i] } 0 <= i && i < |foo.val_array|
    ///     ==> foo.val_array[i].val_ref.val_int == 42```
    /// We would like to unfold the predicate `isize(foo.val_array[i].val_ref)` inside the forall
    /// If we use `Action::Unfold` or `Action::TemporaryUnfold`, and convert them to VIR expression,
    /// we would get the following wrong expression:
    /// ```unfolding acc(isize(foo.val_array[i].val_ref)) in forall ...```
    /// For such cases, one should instead use this variant that will correctly translate the unfolding:
    /// ```forall i: Int :: { foo.val_array[i] } 0 <= i && i < |foo.val_array|
    ///     ==> unfolding acc(isize(foo.val_array[i].val_ref)) in foo.val_array[i].val_ref.val_int == 42```
    QuantifiedUnfold(String, vir::Expr, PermAmount, vir::MaybeEnumVariantIndex),
}

impl Action {
    pub fn to_stmt(&self) -> vir::Stmt {
        match self {
            Action::Fold(ref pred, ref args, perm_amount, ref variant, ref pos) => {
                vir::Stmt::Fold(
                    pred.clone(),
                    args.clone(),
                    *perm_amount,
                    variant.clone(),
                    pos.clone()
                )
            }
            Action::Unfold(ref pred, ref args, perm_amount, ref variant) => {
                vir::Stmt::Unfold(pred.clone(), args.clone(), *perm_amount, variant.clone())
            }
            Action::Drop(..) => vir::Stmt::comment(self.to_string()),
            Action::Assertion(assertion) =>
                vir::Stmt::Assert(assertion.clone(), FoldingBehaviour::Expr, Position::default()),
            Action::TemporaryUnfold(..) =>
                panic!("A temporary unfold has no equivalent in vir::Stmt\n\
                `actions_to_stmts` should be used instead"),
            Action::QuantifiedUnfold(..) =>
                panic!("A quantified unfold has no equivalent in vir::Stmt"),
        }
    }

    pub fn to_expr(&self, inner_expr: vir::Expr) -> vir::Expr {
        match self {
            Action::Fold(ref _pred, ref _args, _perm, ref _variant, _) => {
                // Currently unsupported in Viper
                unimplemented!("action {}", self)
            }

            Action::Unfold(ref pred, ref args, perm, ref variant)
            | Action::TemporaryUnfold(ref pred, ref args, perm, ref variant) => {
                vir::Expr::unfolding(
                    pred.clone(), args.clone(), inner_expr, *perm, variant.clone())
            }

            Action::Drop(..) => inner_expr,

            Action::QuantifiedUnfold(ref pred, ref arg, perm, ref variant) => match inner_expr.clone() {
                vir::Expr::ForAll(vars, triggers, box body, pos) => {
                    assert!(arg.contains_any_var(&vars.iter().cloned().collect()));

                    // We must "push" the unfolding after any possible implications, otherwise
                    // we could end up with the following erroneous expression:
                    // `forall i: Int :: { foo.val_array[i] } unfolding acc(isize(foo.val_array[i].val_ref))
                    //  in 0 <= i && i < |foo.val_array| ==> foo.val_array[i].val_ref.val_int == 42`
                    // The unfolding should happen after `0 <= i && i < |foo.val_array|`
                    let new_body = Self::pushed_unfolding(
                        &vars,
                        pred.clone(),
                        arg.clone(),
                        body,
                        *perm,
                        variant.clone()
                    );
                    vir::Expr::ForAll(vars, triggers, box new_body, pos)
                }
                other => panic!("to_expr of a {} and an inner_expr {} which is not a forall", self, other),
            }
            // TODO: assertion ==> inner_expr can lead to ill-formed expression, maybe panic instead...
            Action::Assertion(_) => inner_expr, // The assertion has already been taken care of.
        }
    }

    fn pushed_unfolding(
        vars: &Vec<vir::LocalVar>,
        pred_name: String,
        arg: vir::Expr,
        body: vir::Expr,
        perm: PermAmount,
        variant: vir::MaybeEnumVariantIndex,
    ) -> vir::Expr {
        let vars = vars.iter().cloned().collect();
        // FIXME This should do for the moment, but may be insufficient for the future.
        //  We should instead "push" the unfolding as long as we do not see any boolean expression
        //  involving `vars`
        match body {
            vir::Expr::BinOp(vir::BinOpKind::Implies, ref lhs, ref rhs, ref pos) if lhs.contains_any_var(&vars) => {
                vir::Expr::BinOp(
                    vir::BinOpKind::Implies,
                    lhs.clone(),
                    box vir::Expr::unfolding(pred_name, vec![arg], *rhs.clone(), perm, variant),
                    pos.clone()
                )
            }
            _ => vir::Expr::unfolding(pred_name, vec![arg], body, perm, variant),
        }
    }
}

/// Converts the actions into two groups of VIR statements, allowing the handling
/// of "temporary unfolds"
/// The first returned vector corresponds to actual actions conversions,
/// while the second contains folds that must be applied once the statement
/// is done.
pub fn actions_to_stmts(actions: Vec<Action>) -> (Vec<vir::Stmt>, Vec<vir::Stmt>) {
    let mut perms = Vec::new();
    let mut to_fold_back = Vec::new();
    for action in actions {
        match action {
            Action::TemporaryUnfold(pred_name, args, perm, variant) => {
                perms.push(vir::Stmt::Unfold(pred_name.clone(), args.clone(), perm, variant.clone()));
                to_fold_back.push(vir::Stmt::Fold(pred_name, args, perm, variant, Position::default()));
            }
            other => perms.push(other.to_stmt()),
        }
    }
    // This reverse is not the most effective...
    (perms, to_fold_back.into_iter().rev().collect())
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Action::Fold(..) | Action::Unfold(..) => write!(f, "{}", self.to_stmt().to_string()),
            Action::Drop(ref perm, ref missing_perm) => {
                write!(f, "drop {} ({})", perm, missing_perm)
            }
            Action::Assertion(assertion) => write!(f, "assert {}", assertion),
            Action::TemporaryUnfold(ref pred_name, ref args, perm, ref variant) =>
                write!(
                    f, "temp-{}",
                    vir::Stmt::Unfold(pred_name.clone(), args.clone(), *perm, variant.clone())
                        .to_string()
                ),
            Action::QuantifiedUnfold(ref pred_name, ref arg, perm, ref variant) =>
                write!(
                    f, "quant-{}",
                    vir::Stmt::Unfold(pred_name.clone(), vec![arg.clone()], *perm, variant.clone())
                        .to_string()
                ),
        }
    }
}
