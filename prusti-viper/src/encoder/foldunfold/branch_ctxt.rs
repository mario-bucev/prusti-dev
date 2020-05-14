// © 2019, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use encoder::foldunfold::action::*;
use encoder::foldunfold::perm::*;
use encoder::foldunfold::places_utils::*;
use encoder::foldunfold::state::*;
use encoder::vir;
use encoder::vir::PermAmount;
use std::collections::HashMap;
use std::collections::HashSet;
use std::iter::FromIterator;
use utils::to_string::ToString;
use std::ops::Try;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCtxt<'a> {
    state: State,
    /// The definition of the predicates
    predicates: &'a HashMap<String, vir::Predicate>,
}

impl<'a> BranchCtxt<'a> {
    pub fn new(
        local_vars: Vec<vir::LocalVar>,
        predicates: &'a HashMap<String, vir::Predicate>,
    ) -> Self {
        BranchCtxt {
            state: State::new(
                HashMap::from_iter(
                    local_vars
                        .into_iter()
                        .map(|v| (vir::Expr::local(v), PermAmount::Write)),
                ),
                HashMap::new(),
                HashSet::new(),
            ),
            predicates,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn mut_state(&mut self) -> &mut State {
        &mut self.state
    }

    pub fn predicates(&self) -> &HashMap<String, vir::Predicate> {
        self.predicates
    }

    /// Simulate an unfold
    fn unfold(
        &mut self,
        pred_place: &vir::Expr,
        perm_amount: PermAmount,
        variant: vir::MaybeEnumVariantIndex,
        // See Action comments on `TemporaryUnfold` for an explanation
        temporary_unfold: bool,
    ) -> Action {
        info!("We want to unfold {} with {}", pred_place, perm_amount);
        //assert!(self.state.contains_acc(pred_place), "missing acc({}) in {}", pred_place, self.state);
        assert!(
            self.state.contains_pred(pred_place),
            "missing pred({}) in {}",
            pred_place,
            self.state
        );
        assert!(
            perm_amount.is_valid_for_specs(),
            "Invalid permission amount."
        );

        let predicate_name = pred_place.typed_ref_name().unwrap();
        let predicate = self.predicates.get(&predicate_name).unwrap();

        let pred_self_place: vir::Expr = predicate.self_place();
        let places_in_pred: Vec<Perm> = predicate
            .get_permissions_with_variant(&variant)
            .into_iter()
            .map(|perm| {
                perm.map_place(|p| p.replace_place(&pred_self_place, pred_place))
                    .update_perm_amount(perm_amount)
            })
            .collect();

        trace!(
            "Pred state before unfold: {{\n{}\n}}",
            self.state.display_pred()
        );

        // Simulate unfolding of `pred_place`
        self.state.remove_pred(&pred_place, perm_amount);
        self.state.insert_all_perms(places_in_pred.into_iter());

        info!("We unfolded {}", pred_place);

        trace!(
            "Acc state after unfold: {{\n{}\n}}",
            self.state.display_acc()
        );
        trace!(
            "Pred state after unfold: {{\n{}\n}}",
            self.state.display_pred()
        );
        trace!(
            "Quant state after unfold: {{\n{}\n}}",
            self.state.display_quant()
        );

        if !temporary_unfold {
            Action::Unfold(
                predicate_name.clone(),
                vec![pred_place.clone().into()],
                perm_amount,
                variant,
            )
        } else {
            Action::TemporaryUnfold(
                predicate_name.clone(),
                vec![pred_place.clone().into()],
                perm_amount,
                variant,
            )
        }
    }

    /// Like `unfold` but deals with quantified predicate access.
    /// This will translate into an 'unfolding in'. See `Action::QuantifiedUnfold` for an example.
    fn unfold_quantified(
        &mut self,
        quant_pred: &vir::QuantifiedResourceAccess,
        perm_amount: PermAmount,
        variant: vir::MaybeEnumVariantIndex,
    ) -> Action {
        debug!("We want to unfold {} with {}", quant_pred, perm_amount);
        assert!(quant_pred.resource.is_pred(), "Quantified resource access must be a predicate");
        assert!(
            self.state.contains_quantified(quant_pred),
            "missing quant_pred({}) in {}",
            quant_pred,
            self.state
        );
        assert!(
            perm_amount.is_valid_for_specs(),
            "Invalid permission amount."
        );

        let predicate_name = quant_pred.resource.get_place().typed_ref_name().unwrap();
        let predicate = self.predicates.get(&predicate_name).unwrap();

        let pred_self_place: vir::Expr = predicate.self_place();
        let quantified_places_in_pred = predicate
            .get_permissions_with_variant(&variant)
            .into_iter()
            .map(|perm| {
                let place = perm.map_place(|p| p.replace_place(&pred_self_place, quant_pred.resource.get_place()))
                    .update_perm_amount(perm_amount);
                let resource = match place {
                    Perm::Acc(place, perm_amount) =>
                        vir::PlainResourceAccess::field(place, perm_amount),
                    Perm::Pred(place, perm_amount) =>
                        vir::PlainResourceAccess::predicate(place, perm_amount).unwrap(),
                    Perm::Quantified(_) => unimplemented!()
                };
                vir::QuantifiedResourceAccess {
                    vars: quant_pred.vars.clone(),
                    triggers: quant_pred.triggers.clone(),
                    cond: quant_pred.cond.clone(),
                    resource
                }
            });

        trace!(
            "Acc state before unfold: {{\n{}\n}}",
            self.state.display_acc()
        );
        trace!(
            "Pred state before unfold: {{\n{}\n}}",
            self.state.display_pred()
        );
        trace!(
            "Quant state before unfold: {{\n{}\n}}",
            self.state.display_quant()
        );

        // Simulate unfolding of `quant_pred`
        self.state.remove_quant(quant_pred);
        // TODO: Can we be sure that these permissions are kept only for the "unfolding in" scope?
        //  They shouldn't be visible afterwards
        self.state.insert_all_quant(quantified_places_in_pred);

        debug!("We unfolded {}", quant_pred.resource.get_place());

        trace!(
            "Acc state after unfold: {{\n{}\n}}",
            self.state.display_acc()
        );
        trace!(
            "Pred state after unfold: {{\n{}\n}}",
            self.state.display_pred()
        );
        trace!(
            "Quant state after unfold: {{\n{}\n}}",
            self.state.display_quant()
        );

        Action::QuantifiedUnfold(
            predicate_name.clone(),
            quant_pred.resource.get_place().clone().into(),
            perm_amount,
            variant,
        )
    }

    /// left is self, right is other
    pub fn join(&mut self, mut other: BranchCtxt) -> (Vec<Action>, Vec<Action>) {
        // All field accs that do not come from a quantified field accesses
        // We treat paths coming from quant. instantiation as if they were
        // not definitely initialized.
        // Note: once the removal of paths that are not definitely initialised
        // is done (cf to-do below) , we should be able to get rid of these two functions.
        fn plain_acc_places(slf: &BranchCtxt) -> HashSet<vir::Expr> {
            slf.state.acc_places()
                .iter()
                .filter(|place| !slf.state.is_acc_an_instance(place))
                .cloned()
                .collect()
        }
        // Similar to `plain_acc_places`, but for predicate instead.
        fn plain_pred_places(slf: &BranchCtxt) -> HashSet<vir::Expr> {
            slf.state.pred_places()
                .iter()
                .filter(|place| !slf.state.is_pred_an_instance(place))
                .cloned()
                .collect()
        }
        let mut left_actions: Vec<Action> = vec![];
        let mut right_actions: Vec<Action> = vec![];

        debug!("Join branches");
        trace!("Left branch: {}", self.state);
        trace!("Right branch: {}", other.state);
        self.state.check_consistency();

        // If they are already the same, avoid unnecessary operations
        if self.state != other.state {
            // Compute which paths are moved out
            /*
            let moved_paths: HashSet<_> = if anti_join {
                filter_with_prefix_in_other(
                    self.state.moved(),
                    other.state.moved()
                )
            } else {
                ancestors(
                    &self.state.moved().clone().union(other.state.moved()).cloned().collect()
                )
            };
            */
            // TODO: Remove all paths that are not definitely initialised.
            let moved_paths: HashSet<_> = ancestors(
                &self
                    .state
                    .moved()
                    .clone()
                    .union(other.state.moved())
                    .cloned()
                    .collect(),
            );
            self.state.set_moved(moved_paths.clone());
            other.state.set_moved(moved_paths.clone());
            debug!("moved_paths: {}", moved_paths.iter().to_string());

            trace!("left acc: {{\n{}\n}}", self.state.display_acc());
            trace!("right acc: {{\n{}\n}}", other.state.display_acc());

            trace!("left pred: {{\n{}\n}}", self.state.display_pred());
            trace!("right pred: {{\n{}\n}}", other.state.display_pred());

            trace!(
                "left acc_leaves: {}",
                self.state.acc_leaves().iter().to_sorted_multiline_string()
            );
            trace!(
                "right acc_leaves: {}",
                other.state.acc_leaves().iter().to_sorted_multiline_string()
            );

            // Compute which access permissions may be preserved
            let (unfold_potential_acc, fold_potential_pred) =
                compute_fold_target(&plain_acc_places(self), &plain_acc_places(&other));
            debug!(
                "unfold_potential_acc: {}",
                unfold_potential_acc.iter().to_sorted_multiline_string()
            );
            debug!(
                "fold_potential_pred: {}",
                fold_potential_pred.iter().to_sorted_multiline_string()
            );

            // Remove access permissions that can not be obtained due to a moved path
            let unfold_actual_acc =
                filter_not_proper_extensions_of(&unfold_potential_acc, &moved_paths);
            debug!(
                "unfold_actual_acc: {}",
                unfold_actual_acc.iter().to_sorted_multiline_string()
            );
            let fold_actual_pred =
                filter_not_proper_extensions_of(&fold_potential_pred, &moved_paths);
            debug!(
                "fold_actual_pred: {}",
                fold_actual_pred.iter().to_sorted_multiline_string()
            );

            // Obtain predicates by folding.
            for pred_place in fold_actual_pred {
                debug!("try to obtain predicate: {}", pred_place);
                let get_perm_amount = |ctxt: &BranchCtxt| {
                    ctxt.state
                        .acc()
                        .iter()
                        .find(|(place, _)| place.has_proper_prefix(&pred_place))
                        .map(|(_, &perm_amount)| perm_amount)
                };
                let perm_amount = get_perm_amount(self)
                    .or_else(|| get_perm_amount(&other))
                    .unwrap();
                let pred_perm = Perm::pred(pred_place.clone(), perm_amount);
                let try_obtain =
                    |left_ctxt: &mut BranchCtxt,
                     right_ctxt: &mut BranchCtxt,
                     left_actions: &mut Vec<_>,
                     right_actions: &mut Vec<_>| {
                        match left_ctxt.obtain(&pred_perm, true) {
                            ObtainResult::Success(new_actions) => {
                                left_actions.extend(new_actions);
                            }
                            ObtainResult::Failure(missing_perm) => {
                                debug!(
                                    "Failed to obtain: {} because of {}",
                                    pred_perm, missing_perm
                                );
                                let remove_places =
                                    |ctxt: &mut BranchCtxt, actions: &mut Vec<_>| {
                                        ctxt.state.remove_moved_matching(|moved_place| {
                                            moved_place.has_prefix(&pred_place)
                                        });
                                        for acc_place in ctxt.state.acc_places() {
                                            if acc_place.has_proper_prefix(&pred_place) {
                                                let perm_amount =
                                                    ctxt.state.remove_acc_place(&acc_place);
                                                let perm = Perm::acc(acc_place, perm_amount);
                                                debug!(
                                                    "dropping perm={} missing_perm={}",
                                                    perm, missing_perm
                                                );
                                                actions
                                                    .push(Action::Drop(perm, missing_perm.clone()));
                                            }
                                        }
                                        for place in ctxt.state.pred_places() {
                                            if place.has_prefix(&pred_place) {
                                                let perm_amount =
                                                    ctxt.state.remove_pred_place(&place);
                                                let perm = Perm::pred(place, perm_amount);
                                                debug!(
                                                    "dropping perm={} missing_perm={}",
                                                    perm, missing_perm
                                                );
                                                actions
                                                    .push(Action::Drop(perm, missing_perm.clone()));
                                            }
                                        }
                                    };
                                remove_places(left_ctxt, left_actions);
                                remove_places(right_ctxt, right_actions);
                            }
                        }
                    };
                try_obtain(self, &mut other, &mut left_actions, &mut right_actions);
                try_obtain(&mut other, self, &mut right_actions, &mut left_actions);
            }
            // Obtain access permissions by unfolding
            for acc_place in &unfold_actual_acc {
                let try_obtain =
                    |ctxt_left: &mut BranchCtxt,
                     ctxt_right: &mut BranchCtxt,
                     left_actions: &mut Vec<_>,
                     right_actions: &mut Vec<_>| {
                        if !ctxt_left.state.acc().contains_key(acc_place) {
                            debug!(
                                "The left branch needs to obtain an access permission: {}",
                                acc_place
                            );
                            let perm_amount = ctxt_right.state.acc()[acc_place];
                            // Unfold something and get `acc_place`
                            let perm = Perm::acc(acc_place.clone(), perm_amount);
                            match ctxt_left.obtain(&perm, true) {
                                ObtainResult::Success(new_actions) => {
                                    left_actions.extend(new_actions);
                                    true
                                }
                                ObtainResult::Failure(missing_perm) => {
                                    ctxt_right.state.remove_perm(&perm);
                                    right_actions.push(Action::Drop(perm, missing_perm));
                                    false
                                }
                            }
                        } else {
                            true
                        }
                    };
                if try_obtain(self, &mut other, &mut left_actions, &mut right_actions) {
                    try_obtain(&mut other, self, &mut right_actions, &mut left_actions);
                }
            }

            // Drop predicate permissions that can not be obtained due to a move
            for pred_place in &filter_proper_extensions_of(&plain_pred_places(self), &moved_paths)
            {
                debug!(
                    "Drop pred {} in left branch (it is moved out in the other branch)",
                    pred_place
                );
                assert!(self.state.pred().contains_key(&pred_place));
                let perm_amount = self.state.remove_pred_place(&pred_place);
                let perm = Perm::pred(pred_place.clone(), perm_amount);
                left_actions.push(Action::Drop(perm.clone(), perm));
            }
            for pred_place in &filter_proper_extensions_of(&plain_pred_places(&other), &moved_paths)
            {
                debug!(
                    "Drop pred {} in right branch (it is moved out in the other branch)",
                    pred_place
                );
                assert!(other.state.pred().contains_key(&pred_place));
                let perm_amount = other.state.remove_pred_place(&pred_place);
                let perm = Perm::pred(pred_place.clone(), perm_amount);
                right_actions.push(Action::Drop(perm.clone(), perm));
            }

            // Compute preserved predicate permissions
            let preserved_preds: HashSet<_> =
                intersection(&plain_pred_places(self), &plain_pred_places(&other));
            debug!("preserved_preds: {}", preserved_preds.iter().to_string());

            // Drop predicate permissions that are not in the other branch
            for pred_place in plain_pred_places(self).difference(&preserved_preds) {
                debug!(
                    "Drop pred {} in left branch (it is not in the other branch)",
                    pred_place
                );
                assert!(self.state.pred().contains_key(&pred_place));
                let perm_amount = self.state.remove_pred_place(&pred_place);
                let perm = Perm::pred(pred_place.clone(), perm_amount);
                left_actions.push(Action::Drop(perm.clone(), perm));
            }
            for pred_place in plain_pred_places(&other).difference(&preserved_preds) {
                debug!(
                    "Drop pred {} in right branch (it is not in the other branch)",
                    pred_place
                );
                assert!(other.state.pred().contains_key(&pred_place));
                let perm_amount = other.state.remove_pred_place(&pred_place);
                let perm = Perm::pred(pred_place.clone(), perm_amount);
                right_actions.push(Action::Drop(perm.clone(), perm));
            }

            // Drop access permissions that can not be obtained due to a move
            for acc_place in &filter_proper_extensions_of(&plain_acc_places(self), &moved_paths) {
                debug!(
                    "Drop acc {} in left branch (it is moved out in the other branch)",
                    acc_place
                );
                assert!(self.state.acc().contains_key(&acc_place));
                let perm_amount = self.state.remove_acc_place(&acc_place);
                let perm = Perm::acc(acc_place.clone(), perm_amount);
                left_actions.push(Action::Drop(perm.clone(), perm));
            }
            for acc_place in &filter_proper_extensions_of(&plain_acc_places(&other), &moved_paths) {
                debug!(
                    "Drop acc {} in right branch (it is moved out in the other branch)",
                    acc_place
                );
                assert!(other.state.acc().contains_key(&acc_place));
                let perm_amount = other.state.remove_acc_place(&acc_place);
                let perm = Perm::acc(acc_place.clone(), perm_amount);
                right_actions.push(Action::Drop(perm.clone(), perm));
            }

            // Drop access permissions not in `actual_acc`
            for acc_place in plain_acc_places(self).difference(&plain_acc_places(&other)) {
                debug!(
                    "Drop acc {} in left branch (not present in the other branch)",
                    acc_place
                );
                assert!(self.state.acc().contains_key(&acc_place));
                let perm_amount = self.state.remove_acc_place(&acc_place);
                self.state
                    .remove_moved_matching(|moved_place| moved_place.has_prefix(acc_place));
                let perm = Perm::acc(acc_place.clone(), perm_amount);
                left_actions.push(Action::Drop(perm.clone(), perm));
            }
            for acc_place in plain_acc_places(&other).difference(&plain_acc_places(self)) {
                debug!(
                    "Drop acc {} in right branch (not present in the other branch)",
                    acc_place
                );
                assert!(other.state.acc().contains_key(&acc_place));
                let perm_amount = other.state.remove_acc_place(&acc_place);
                other
                    .state
                    .remove_moved_matching(|moved_place| moved_place.has_prefix(acc_place));
                let perm = Perm::acc(acc_place.clone(), perm_amount);
                right_actions.push(Action::Drop(perm.clone(), perm));
            }

            // If we have `Read` and `Write`, make both `Read`.
            for acc_place in plain_acc_places(self) {
                assert!(other.state.acc().contains_key(&acc_place)
                        "acc_place = {}", acc_place);
                let left_perm = self.state.acc()[&acc_place];
                let right_perm = other.state.acc()[&acc_place];
                if left_perm == PermAmount::Write && right_perm == PermAmount::Read {
                    self.state.remove_acc(&acc_place, PermAmount::Remaining);
                    let perm = Perm::acc(acc_place.clone(), PermAmount::Remaining);
                    left_actions.push(Action::Drop(perm.clone(), perm));
                }
                if left_perm == PermAmount::Read && right_perm == PermAmount::Write {
                    other.state.remove_acc(&acc_place, PermAmount::Remaining);
                    let perm = Perm::acc(acc_place.clone(), PermAmount::Remaining);
                    right_actions.push(Action::Drop(perm.clone(), perm));
                }
            }
            for pred_place in plain_pred_places(self) {
                assert!(other.state.pred().contains_key(&pred_place));
                let left_perm = self.state.pred()[&pred_place];
                let right_perm = other.state.pred()[&pred_place];
                if left_perm == PermAmount::Write && right_perm == PermAmount::Read {
                    self.state.remove_pred(&pred_place, PermAmount::Remaining);
                    let perm = Perm::pred(pred_place.clone(), PermAmount::Remaining);
                    left_actions.push(Action::Drop(perm.clone(), perm));
                }
                if left_perm == PermAmount::Read && right_perm == PermAmount::Write {
                    other.state.remove_pred(&pred_place, PermAmount::Remaining);
                    let perm = Perm::pred(pred_place.clone(), PermAmount::Remaining);
                    right_actions.push(Action::Drop(perm.clone(), perm));
                }
            }

            let drop_quantified =
                |ctxt_left: &mut BranchCtxt,
                 ctxt_right: &mut BranchCtxt,
                 left_actions: &mut Vec<_>,
                 right_actions: &mut Vec<_>| {
                    for left_quant in ctxt_left.state.quantified().clone().into_iter() {
                        match ctxt_right.state.get_quantified(&left_quant, false).cloned() {
                            Some(right_quant) => {
                                let left_perm = left_quant.get_perm_amount();
                                let right_perm = right_quant.get_perm_amount();
                                if left_perm == PermAmount::Write && right_perm == PermAmount::Read {
                                    let to_remove = left_quant.clone()
                                        .update_perm_amount(PermAmount::Remaining);
                                    ctxt_left.state.remove_quant(&to_remove);
                                    let perm = Perm::quantified(to_remove);
                                    left_actions.push(Action::Drop(perm.clone(), perm));
                                }
                                if left_perm == PermAmount::Read && right_perm == PermAmount::Write {
                                    let to_remove = left_quant.clone()
                                        .update_perm_amount(PermAmount::Remaining);
                                    ctxt_right.state.remove_quant(&to_remove);
                                    let perm = Perm::quantified(to_remove);
                                    right_actions.push(Action::Drop(perm.clone(), perm));
                                }
                            }
                            None => {
                                ctxt_left.state.remove_quant(&left_quant);
                                let perm = Perm::quantified(left_quant);
                                left_actions.push(Action::Drop(perm.clone(), perm));
                            }
                        }
                    }
                };
            drop_quantified(self, &mut other, &mut left_actions, &mut right_actions);
            drop_quantified(&mut other, self, &mut right_actions, &mut left_actions);

            trace!(
                "Actions in left branch: \n{}",
                left_actions
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(",\n")
            );
            trace!(
                "Actions in right branch: \n{}",
                right_actions
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(",\n")
            );

            assert_eq!(plain_acc_places(self), plain_acc_places(&other));
            assert_eq!(plain_pred_places(self), plain_pred_places(&other));
            assert_eq!(self.state.quantified(), other.state.quantified());
            self.state.check_consistency();
        }

        return (left_actions, right_actions);
    }

    /// Obtain the required permissions, changing the state inplace and returning the statements.
    fn obtain_all(&mut self, reqs: Vec<Perm>) -> Vec<Action> {
        debug!("[enter] obtain_all: {{{}}}", reqs.iter().to_string());
        reqs.iter()
            .flat_map(|perm| self.obtain(perm, false).unwrap())
            .collect()
    }

    /// Obtain the required permission, changing the state inplace and returning the statements.
    ///
    /// ``in_join`` – are we currently trying to join branches?
    fn obtain(&mut self, req: &Perm, in_join: bool) -> ObtainResult {
        info!("[enter] obtain(req={})", req);
        let quant_vars = match req {
            Perm::Quantified(quant) => quant.vars.iter().cloned().collect(),
            _ => HashSet::new()
        };
        // First, obtain permissions of all prefixes
        let mut prefixes = req.get_place()
            .all_proper_prefixes()
            .into_iter()
            // We do not want to include prefixes containing quantified variables
            // because it does not make sense to obtain a permission over such prefixes
            .take_while(|prefix| !prefix.contains_any_var(&quant_vars));
        let mut proper_places_actions = prefixes
            .try_fold(
                Vec::<Action>::new(),
                |mut actions, place| {
                    let sub_req = Perm::Acc(place, req.get_perm_amount());
                    let new_actions =
                        self.do_obtain(&sub_req, in_join).into_result()?;
                    actions.extend(new_actions);
                    Ok(actions)
                }
            )?;
        // Then obtain the actual permission
        proper_places_actions.extend(self.do_obtain(&req, in_join)?);
        ObtainResult::Success(proper_places_actions)
    }

    // Actual implementation for obtaining the permissions
    fn do_obtain(&mut self, req: &Perm, in_join: bool) -> ObtainResult {
        info!("[enter] do_obtain(req={})", req);

        let mut actions: Vec<Action> = vec![];

        info!("Acc state before: {{\n{}\n}}", self.state.display_acc());
        info!("Pred state before: {{\n{}\n}}", self.state.display_pred());
        info!("Quant. state before: {{\n{}\n}}", self.state.display_quant());

        // 1. Check if the requirement is satisfied
        if self.state.contains_perm(req) {
            info!("[exit] do_obtain: Requirement {} is satisfied", req);
            return ObtainResult::Success(actions);
        }
        // If the request is quantified, we may actually have a quantified resource access
        // that "looks the same" but with different preconditions.
        // e.g. we could have a req of `forall i :: 0 < i < 12 ==> acc(foo.val_array[i].val_ref)`
        // and have in our permissions set `forall i :: 0 < i < 32 ==> acc(foo.val_array[i].val_ref)`
        // In this case, we will assert that `forall i :: 0 < i < 12 ==> 0 < i < 32` and
        // return success with this assertion.
        if let Perm::Quantified(quant) = req {
            if let Some((matched_quant, mapping_result)) = self.state.contains_quantified_ignoring_preconds(quant) {
                // This cannot happen because we would have returned with `self.state.contains_perm`.
                assert!(!mapping_result.identical_cond);
                info!(
                    "Mismatch between the preconditions of {} (request) and {} (matched quant.)",
                    quant,
                    matched_quant
                );
                actions.push(
                    Action::Assertion(
                        vir::Expr::forall(
                            // We use the matched quant vars, and rename the request vars accordingly
                            matched_quant.vars.clone(),
                            vec![],
                            vir::Expr::implies(
                                quant.cond.clone().rename(&mapping_result.vars_mapping),
                                *matched_quant.cond
                            )
                        )
                    )
                );
                info!("[exit] do_obtain: Requirement {} is satisfied", req);
                return ObtainResult::Success(actions);
            }
        }

        if req.is_acc() && req.is_local() {
            // access permissions on local variables are always satisfied
            trace!("[exit] do_obtain: Requirement {} is satisfied", req);
            return ObtainResult::Success(actions);
        }

        info!("Try to satisfy requirement {}", req);

        // 3. Obtain with an unfold
        match req {
            // Things differ a bit whether the req is quantified or not, but the idea is the same
            Perm::Acc(..) | Perm::Pred(..) => {
                // Find a predicate on a proper prefix of req
                let existing_prefix_pred_opt: Option<vir::Expr> = self
                    .state
                    .pred_places()
                    .iter()
                    .find(|p| req.has_proper_prefix(p))
                    .cloned();
                if let Some(existing_pred_to_unfold) = existing_prefix_pred_opt {
                    let perm_amount = self.state.pred()[&existing_pred_to_unfold];
                    info!(
                        "We want to unfold {} with permission {} (we need at least {})",
                        existing_pred_to_unfold,
                        perm_amount,
                        req.get_perm_amount()
                    );
                    assert!(perm_amount >= req.get_perm_amount());
                    let variant = self.find_variant(&existing_pred_to_unfold, req.get_place());
                    let action = self.unfold(&existing_pred_to_unfold, perm_amount, variant, false);
                    actions.push(action);
                    info!("We unfolded {}", existing_pred_to_unfold);

                    // Check if we are done
                    let new_actions = self.do_obtain(req, false).or_else(|_| ObtainResult::Failure(req.clone()))?;
                    actions.extend(new_actions);
                    info!("[exit] do_obtain");
                    return ObtainResult::Success(actions);
                }
            }
            Perm::Quantified(quant) => {
                let existing_prefix_quant_pred_opt = self
                    .state
                    .quantified()
                    .iter()
                    .filter(|p| p.resource.is_pred())
                    .filter_map(|p| quant.has_proper_prefix(p).map(|res| (p.clone(), res)))
                    .next();
                if let Some((existing_quant_pred_to_unfold, proper_prefix_res)) = existing_prefix_quant_pred_opt {
                    let perm_amount = existing_quant_pred_to_unfold.get_perm_amount();
                    info!(
                        "We want to unfold {} with permission {} (we need at least {})",
                        existing_quant_pred_to_unfold,
                        perm_amount,
                        req.get_perm_amount()
                    );
                    assert!(perm_amount >= req.get_perm_amount());
                    let variant = self.find_variant(&existing_quant_pred_to_unfold.resource.get_place(), req.get_place());
                    let action = self.unfold_quantified(&existing_quant_pred_to_unfold, perm_amount, variant);
                    actions.push(action);
                    info!("We unfolded {}", existing_quant_pred_to_unfold);
                    let new_req = {
                        if proper_prefix_res.identical_cond {
                            req.clone()
                        } else {
                            // The preconditions aren't the same, so we will just assert that the
                            // preconds of the request implies the preconds of what we have (existing_quant_pred_to_unfold).
                            // Then, we will replace the preconds of the request with the preconds
                            // of existing_quant_pred_to_unfold for the recursive call.
                            info!(
                                "Mismatch between the preconditions of {} (request) and {} (unfolded)",
                                quant,
                                existing_quant_pred_to_unfold
                            );
                            // TODO: perform renaming
                            actions.push(
                                Action::Assertion(
                                    vir::Expr::forall(
                                        // We use existing_quant_pred_to_unfold vars,
                                        // and rename the request vars accordingly
                                        existing_quant_pred_to_unfold.vars.clone(),
                                        vec![],
                                        vir::Expr::implies(
                                            *quant.cond.clone(),
                                            *existing_quant_pred_to_unfold.cond.clone()
                                        )
                                    )
                                )
                            );
                            let mut new_quant = quant.clone();
                            // TODO: perform renaming
                            new_quant.cond = existing_quant_pred_to_unfold.cond.clone();
                            Perm::Quantified(new_quant)
                        }
                    };

                    // Check if we are done
                    let new_actions = self.do_obtain(&new_req, false).or_else(|_| ObtainResult::Failure(req.clone()))?;
                    actions.extend(new_actions);
                    trace!("[exit] do_obtain");
                    return ObtainResult::Success(actions);
                }
            }
        }

        // 4. Obtain with a fold
        if req.is_pred() {
            // We want to fold `req`
            info!("We want to fold {}", req);
            let predicate_name = req.typed_ref_name().unwrap();
            let predicate = self.predicates.get(&predicate_name).unwrap();

            let variant = self.find_fold_variant(req);

            // Find an access permission for which req is a proper suffix
            let existing_proper_perm_extension_opt: Option<_> = self
                .state
                .acc_places()
                .into_iter()
                .find(|p| p.has_proper_prefix(req.get_place()));

            let pred_self_place: vir::Expr = predicate.self_place();
            let places_in_pred: Vec<Perm> = predicate
                .get_permissions_with_variant(&variant)
                .into_iter()
                .map(|perm| perm.map_place(|p| p.replace_place(&pred_self_place, req.get_place())))
                .flat_map(|p| match p {
                    Perm::Acc(..) | Perm::Pred(..) =>
                        vec![p],
                    Perm::Quantified(quant) => {
                        let mut perms = Vec::new();
                        if quant.resource.is_field_acc() {
                            // We go over all fields acc and add the ones that comes
                            // from this quantified field access.
                            for (acc, acc_perm) in self.state.acc().clone() {
                                if let Some(instance) = quant.try_instantiate(&acc) {
                                    if instance.is_match_perfect() {
                                        assert!(instance.instantiated().resource.is_field_acc());
                                        perms.push(Perm::Acc(acc, acc_perm));
                                    }
                                }
                            }
                        } else {
                            // else: is a predicate access
                            // We do the same for pred accs
                            for (pred, pred_perm) in self.state.pred().clone() {
                                if let Some(instance) = quant.try_instantiate(&pred) {
                                    if instance.is_match_perfect() {
                                        assert!(instance.instantiated().resource.is_pred());
                                        perms.push(Perm::Pred(pred, pred_perm));
                                    }
                                }
                            }
                            // We may have unfolded a quantified predicate instance.
                            // As an example, suppose we have the quant. pred.
                            // forall i :: (cond) => isize(self.val_array[i].val_ref)
                            // and an instantiation:
                            // isize(_1.val_ref.val_array[idx].val_ref)
                            // If we unfolded this predicate, we would have
                            // acc(_1.val_ref.val_array[idx].val_ref.val_int)
                            // which ends up in state.acc().
                            // To determine whether we unfolded this quant. pred., we
                            // search for proper suffixes of the quant. pred. by
                            // looking at the accs.
                            // In the example, we would find that
                            // _1.val_ref.val_array[idx].val_ref.val_int
                            // can be instantiated from isize(_1.val_ref.val_array[idx].val_ref)
                            // so we add isize(_1.val_ref.val_array[idx].val_ref) into the perms
                            // to be obtained (i.e., we need to fold _1.val_ref.val_array[idx].val_ref.val_int).
                            for (acc, acc_perm) in self.state.acc().clone() {
                                if let Some(instance) = quant.try_instantiate(&acc) {
                                    if instance.match_type() == vir::InstantiationResultMatchType::PrefixPredAccMatch {
                                        assert!(instance.instantiated().resource.is_pred());
                                        // We indeed push the proper prefix (instance.(..).resource) and not the acc itself
                                        // as noted in the example above.
                                        perms.push(Perm::Pred(instance.into_instantiated().resource.into_place(), acc_perm));
                                        break;
                                    }
                                }
                            }
                        }
                        perms
                    }
                })
                .collect();

            // Check that there exists something that would make the fold possible.
            // We don't want to end up in an infinite recursion, trying to obtain the
            // predicates in the body.
            let can_fold = match existing_proper_perm_extension_opt {
                Some(_) => true,
                None => places_in_pred.is_empty() && !predicate.is_abstract(),
            };

            if can_fold {
                let perm_amount = places_in_pred
                    .iter()
                    .map(|p| {
                        self.state
                            .acc()
                            .iter()
                            .chain(self.state.pred().iter())
                            .filter(|(place, _)| place.has_prefix(p.get_place()))
                            .map(|(place, perm_amount)| {
                                debug!("Place {} can offer {}", place, perm_amount);
                                *perm_amount
                            })
                            .min()
                            .unwrap_or(PermAmount::Write)
                    })
                    .min()
                    .unwrap_or(PermAmount::Write);
                info!(
                    "We want to fold {} with permission {} (we need at least {})",
                    req,
                    perm_amount,
                    req.get_perm_amount()
                );

                for fold_req_place in &places_in_pred {
                    let pos = req.get_place().pos().clone();
                    let new_req_place = fold_req_place.clone().set_default_pos(pos);
                    let obtain_result = self.do_obtain(&new_req_place, false);
                    match obtain_result {
                        ObtainResult::Success(new_actions) => {
                            actions.extend(new_actions);
                        }
                        ObtainResult::Failure(_) => {
                            return obtain_result;
                        }
                    }
                }

                let scaled_places_in_pred: Vec<_> = places_in_pred
                    .into_iter()
                    .map(|perm| perm.update_perm_amount(perm_amount))
                    .collect();
                // Scale or remove quantified predicates that have been unfolded
                let scaled_quantified: Vec<_> = self.state
                    .get_quantified_resources_suffixes_of(req.get_place())
                    .into_iter()
                    .map(|quant| Perm::Quantified(quant.update_perm_amount(perm_amount)))
                    .collect();

                let pos = req.get_place().pos().clone();
                let fold_action = Action::Fold(
                    predicate_name.clone(),
                    vec![req.get_place().clone().into()],
                    perm_amount,
                    variant,
                    pos,
                );
                actions.push(fold_action);

                // Simulate folding of `req`
                assert!(self.state.contains_all_perms(scaled_places_in_pred.iter()));
                assert!(
                    !req.get_place().is_simple_place() || self.state.contains_acc(req.get_place()),
                    "req={} state={}",
                    req.get_place(),
                    self.state
                );
                assert!(!self.state.contains_pred(req.get_place()));
                self.state.remove_all_perms(scaled_places_in_pred.iter());
                self.state.remove_all_perms(scaled_quantified.iter());
                self.state.insert_pred(req.get_place().clone(), perm_amount);

                // Done. Continue checking the remaining requirements
                info!("We folded {}", req);
                info!("[exit] obtain");
                return ObtainResult::Success(actions);
            }
            // else: cannot fold, so we fallthrough.
        }

        // 5. Obtain from a quantified resource
        let all_instances = self.state.get_all_quantified_instances(req);
        match self.handle_quantified_instances_results(req, all_instances) {
            ObtainResult::Success(new_actions) => {
                actions.extend(new_actions);
                ObtainResult::Success(actions)
            }
            ObtainResult::Failure(_) if in_join && req.get_perm_amount() == vir::PermAmount::Read => {
                // Permissions held by shared references can be dropped
                // without being explicitly moved becauce &T implements Copy.
                ObtainResult::Failure(req.clone())
            }
            ObtainResult::Failure(_) => {
                info!(
                    r"There is no access permission to obtain {} ({:?}).
Access permissions: {{
{}
}}
Predicates: {{
{}
}}
Quantified: {{
{}
}}
",
                    req,
                    req,
                    self.state.display_acc(),
                    self.state.display_pred(),
                    self.state.display_quant(),
                );
                ObtainResult::Failure(req.clone())
            }
        }
    }

    fn handle_quantified_instances_results(
        &mut self,
        req: &Perm,
        inst_results: Vec<vir::InstantiationResult>
    ) -> ObtainResult {
        debug!(
            "[enter] handle_quantified_instances_results\n\
            eq = {}\n\
            access_results = {}\n\
            state = {}",
            req,
            inst_results.iter()
                .map(|res| res.instantiated().to_string())
                .collect::<Vec<String>>()
                .join(", "),
            self.state,
        );
        inst_results.into_iter()
            .map(|res| self.handle_quantified_instances_result(req, res))
            .find(|obtain_res| obtain_res.is_success())
            .unwrap_or_else(|| ObtainResult::Failure(req.clone()))
    }

    fn handle_quantified_instances_result(
        &mut self,
        req: &Perm,
        inst_result: vir::InstantiationResult
    ) -> ObtainResult {
        use encoder::vir::InstantiationResultMatchType::*;
        debug!(
            "handle_quantified_instances_result req {} --> {}  {:?}",
            req, inst_result.instantiated(), inst_result.match_type()
        );
        let match_type = inst_result.match_type();
        let quant = inst_result.into_instantiated();
        let precond = *quant.cond;
        if quant.resource.get_perm_amount() < req.get_perm_amount() {
            return ObtainResult::Failure(req.clone());
        }

        let perm_amount = quant.resource.get_perm_amount();
        let mut actions = Vec::new();
        debug!("handle_quantified_instances_result: \
        Requirement {} may be satisfied only if {} is satisfied", req, precond);
        // Since quantified resource access have the form `forall vars :: cond ==> resource`,
        // we need to satisfy `cond` (e.g. index range for array access).
        // We simply assert them.
        actions.push(Action::Assertion(precond));
        // We have instantiated a quant. resource, but there are different type of "matching"
        // that will determine whether we need to do more work or not
        match match_type {
            // We have asked for e.g. `acc(a.b[x].d)` and the instantiation gave us
            // exactly that (i.e., the quant. field. was of the form `acc(a.b[i].d)`)
            // In that case, we are done.
            PerfectFieldAccMatch => {
                assert!(req.is_acc());
                assert_eq!(req.get_place(), quant.resource.get_place());
                self.state.insert_perm(req.clone().update_perm_amount(perm_amount));
                ObtainResult::Success(actions)
            }
            // Similarly to the previous case, we asked e.g. `acc(isize(a.b[x].d))` and
            // the instantiation gave us exactly that.
            PerfectPredAccMatch if req.is_pred() => {
                assert_eq!(req.get_place(), quant.resource.get_place());
                self.state.insert_perm(req.clone().update_perm_amount(perm_amount));
                ObtainResult::Success(actions)
            }
            // We have asked for e.g. `acc(a.b[x].d)` and the instantiation gave
            // us `acc(isize(a.b[x].d))`. Such instantiation is somewhat
            // ill-formed: we can't obtain `acc(a.b[x].d)` by unfolding `size(a.b[x].d)`
            // (indeed, to unfold `isize(a.b[x].d)`, we actually need `acc(a.b[x].d)`).
            PerfectPredAccMatch => {
                assert!(req.is_acc());
                ObtainResult::Failure(req.clone())
            }
            // We have asked for e.g. `acc(isize(a.b[x].d.e))` and the instantiation gave
            // us `acc(isize(a.b[x].d))` (`.e` missing). In that case, we give up
            // and hope that the next instantiation will be more successful.
            PrefixPredAccMatch if req.is_pred() => {
                ObtainResult::Failure(req.clone())
            }
            // We have asked for e.g. `acc(a.b[x].d.e)` and the instantiation gave us
            // e.g. `acc(isize(a.b[x].d))`. So we try to obtain this permission
            // by unfolding `isize(a.b[x].d)`
            PrefixPredAccMatch => {
                assert!(req.is_acc());
                let predicate = match quant.resource {
                    vir::PlainResourceAccess::Predicate(pred) => pred,
                    // The instantiation says we have matched against a predicate instance,
                    // so the quantified resource must be a predicate!
                    _ => unreachable!(),
                };
                // Indeed, since predicate is extracted from quant.resource
                assert_eq!(predicate.perm, perm_amount);
                self.state.insert_pred(*predicate.arg.clone(), predicate.perm);
                actions.push(self.unfold(&*predicate.arg, predicate.perm, None, true));
                // Try to obtain the resource again
                actions.extend(self.do_obtain(&req.clone().update_perm_amount(perm_amount), false)?);
                ObtainResult::Success(actions)
            }
            // Obtaining a prefix match on field is useless in any case.
            PrefixFieldAccMatch => {
                ObtainResult::Failure(req.clone())
            }
        }
    }

    /// Returns some of the dropped permissions
    pub fn apply_stmt(&mut self, stmt: &vir::Stmt) {
        debug!("apply_stmt: {}", stmt);

        trace!("Acc state before: {{\n{}\n}}", self.state.display_acc());
        trace!("Pred state before: {{\n{}\n}}", self.state.display_pred());
        trace!("Quant. state before: {{\n{}\n}}", self.state.display_quant());

        self.state.check_consistency();

        stmt.apply_on_state(&mut self.state, self.predicates);

        trace!("Acc state after: {{\n{}\n}}", self.state.display_acc());
        trace!("Pred state after: {{\n{}\n}}", self.state.display_pred());
        trace!("Quant. state after: {{\n{}\n}}", self.state.display_quant());

        self.state.check_consistency();
    }

    pub fn obtain_permissions(&mut self, permissions: Vec<Perm>) -> Vec<Action> {
        trace!(
            "[enter] obtain_permissions: {}",
            permissions.iter().to_string()
        );

        trace!("Acc state before: {{\n{}\n}}", self.state.display_acc());
        trace!("Pred state before: {{\n{}\n}}", self.state.display_pred());
        trace!("Quant. state before: {{\n{}\n}}", self.state.display_quant());

        self.state.check_consistency();

        let actions = self.obtain_all(permissions);

        trace!("Acc state after: {{\n{}\n}}", self.state.display_acc());
        trace!("Pred state after: {{\n{}\n}}", self.state.display_pred());
        trace!("Quant. state after: {{\n{}\n}}", self.state.display_quant());

        self.state.check_consistency();

        trace!("[exit] obtain_permissions: {}", actions.iter().to_string());
        actions
    }

    /// Find the variant of enum that `place` has.
    fn find_variant(
        &self,
        place: &vir::Expr,
        prefixed_place: &vir::Expr
    ) -> vir::MaybeEnumVariantIndex {
        trace!("[enter] find_variant(place={}, prefixed_place={})", place, prefixed_place);
        let parent = prefixed_place.get_parent_ref().unwrap();
        let result = if place == parent {
            match prefixed_place {
                vir::Expr::Variant(_, field, _) => {
                    Some(field.into())
                },
                _ => {
                    None
                }
            }
        } else {
            self.find_variant(place, parent)
        };
        trace!("[exit] find_variant(place={}, prefixed_place={}) = {:?}",
               place, prefixed_place, result);
        result
    }

    /// Find the variant of enum that should be folded.
    fn find_fold_variant(&self, req: &Perm) -> vir::MaybeEnumVariantIndex {
        let req_place = req.get_place();
        // Find an access permission for which req is a proper suffix and extract variant from it.
        self.state
            .acc_places()
            .into_iter()
            .find(|place| {
                place.has_proper_prefix(req_place) && place.is_variant()
            })
            .and_then(|prefixed_place| {
                self.find_variant(req_place, &prefixed_place)
            })
    }
}

/// Computes a pair of sets of places that should be obtained. The first
/// element of the pair is the set of places that should be obtained by
/// unfolding while the second element should be obtained by folding.
///
/// The first set is computed by taking the elements that have a prefix
/// in another set. For example:
///
/// ```plain
///   { a, b.c, d.e.f, d.g },
///   { a, b.c.d, b.c.e, d.e,h }
/// ```
///
/// becomes:
///
/// ```plain
/// { a, b.c.d, b.c.e, d.e.f }
/// ```
///
/// The second set is the set of enums that are unfolded differently in
/// input sets.
///
/// The elements from the first set that have any element in the second
/// set as a prefix are dropped.
pub fn compute_fold_target(
    left: &HashSet<vir::Expr>,
    right: &HashSet<vir::Expr>,
) -> (HashSet<vir::Expr>, HashSet<vir::Expr>) {
    let mut conflicting_base = HashSet::new();
    // If we have an enum unfolded only in one, then we add that enum to
    // conflicting places.
    let mut conflicting_base_check = |item: &vir::Expr, second_set: &HashSet<vir::Expr>| {
        if let vir::Expr::Variant(box ref base, _, _) = item {
            if !second_set.iter().any(|p| p.has_prefix(item)) {
                // The enum corresponding to base is completely folded in second_set or unfolded
                // with a different variant.
                conflicting_base.insert(base.clone());
            }
        }
    };
    for left_item in left.iter() {
        conflicting_base_check(left_item, right);
    }
    for right_item in right.iter() {
        conflicting_base_check(right_item, left);
    }

    let mut places = HashSet::new();
    let mut place_check = |item: &vir::Expr, item_set: &HashSet<vir::Expr>,
                                            other_set: &HashSet<vir::Expr>| {
        let is_leaf = !item_set.iter().any(|p| p.has_proper_prefix(item));
        let below_all_others = !other_set.iter().any(|p| p.has_prefix(item));
        let no_conflict_base = !conflicting_base.iter().any(|base| item.has_prefix(base));
        if is_leaf && below_all_others && no_conflict_base {
            places.insert(item.clone());
        }
    };
    for left_item in left.iter() {
        place_check(left_item, left, right);
    }
    for right_item in right.iter() {
        place_check(right_item, right, left);
    }

    let acc_places = places;
    let pred_places: HashSet<_> = conflicting_base
        .iter()
        .filter(|place| {
            !conflicting_base
                .iter()
                .any(|base| place.has_proper_prefix(base))
        })
        .cloned()
        .collect();
    (acc_places, pred_places)
}

/// Result of the obtain operation. Either success and a list of actions, or failure and the
/// permission that was missing.
enum ObtainResult {
    Success(Vec<Action>),
    Failure(Perm),
}

impl ObtainResult {
    pub fn unwrap(self) -> Vec<Action> {
        match self {
            ObtainResult::Success(actions) => actions,
            ObtainResult::Failure(_) => unreachable!(),
        }
    }

    pub fn is_success(&self) -> bool {
        match self {
            ObtainResult::Success(_) => true,
            ObtainResult::Failure(_) => false,
        }
    }

    pub fn or_else<F>(self, on_failure: F) -> Self
        where F: FnOnce(Perm) -> Self
    {
        match self {
            ObtainResult::Success(v) => ObtainResult::Success(v),
            ObtainResult::Failure(p) => on_failure(p),
        }
    }
}

impl Try for ObtainResult {
    type Ok = Vec<Action>;
    type Error = Perm;

    fn into_result(self) -> Result<Self::Ok, Self::Error> {
        match self {
            ObtainResult::Success(v) => Ok(v),
            ObtainResult::Failure(p) => Err(p)
        }
    }

    fn from_error(p: Self::Error) -> Self {
        ObtainResult::Failure(p)
    }

    fn from_ok(v: Self::Ok) -> Self {
        ObtainResult::Success(v)
    }
}