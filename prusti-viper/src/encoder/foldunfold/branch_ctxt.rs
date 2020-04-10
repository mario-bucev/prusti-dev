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
use encoder::vir::{PermAmount, ResourceAccessResult, ResourceAccess};
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
        let places_in_pred = predicate
            .get_available_permissions_with_variant(&variant)
            .into_iter()
            .map(|perm| {
                perm.map_place(|p| p.replace_place(&pred_self_place, pred_place))
                    .update_perm_amount(perm_amount)
            });

        trace!(
            "Pred state before unfold: {{\n{}\n}}",
            self.state.display_pred()
        );

        // Simulate unfolding of `pred_place`
        self.state.remove_pred(&pred_place, perm_amount);
        self.state.insert_all_available_perms(places_in_pred);

        debug!("We unfolded {}", pred_place);

        trace!(
            "Acc state after unfold: {{\n{}\n}}",
            self.state.display_acc()
        );
        trace!(
            "Pred state after unfold: {{\n{}\n}}",
            self.state.display_pred()
        );

        Action::Unfold(
            predicate_name.clone(),
            vec![pred_place.clone().into()],
            perm_amount,
            variant,
        )
    }

    /// left is self, right is other
    pub fn join(&mut self, mut other: BranchCtxt) -> (Vec<Action>, Vec<Action>) {
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
                compute_fold_target(&self.state.acc_places(), &other.state.acc_places());
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
            for pred_place in &filter_proper_extensions_of(&self.state.pred_places(), &moved_paths)
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
            for pred_place in &filter_proper_extensions_of(&other.state.pred_places(), &moved_paths)
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
                intersection(&self.state.pred_places(), &other.state.pred_places());
            debug!("preserved_preds: {}", preserved_preds.iter().to_string());

            // Drop predicate permissions that are not in the other branch
            for pred_place in self.state.pred_places().difference(&preserved_preds) {
                debug!(
                    "Drop pred {} in left branch (it is not in the other branch)",
                    pred_place
                );
                assert!(self.state.pred().contains_key(&pred_place));
                let perm_amount = self.state.remove_pred_place(&pred_place);
                let perm = Perm::pred(pred_place.clone(), perm_amount);
                left_actions.push(Action::Drop(perm.clone(), perm));
            }
            for pred_place in other.state.pred_places().difference(&preserved_preds) {
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
            for acc_place in &filter_proper_extensions_of(&self.state.acc_places(), &moved_paths) {
                debug!(
                    "Drop acc {} in left branch (it is moved out in the other branch)",
                    acc_place
                );
                assert!(self.state.acc().contains_key(&acc_place));
                let perm_amount = self.state.remove_acc_place(&acc_place);
                let perm = Perm::acc(acc_place.clone(), perm_amount);
                left_actions.push(Action::Drop(perm.clone(), perm));
            }
            for acc_place in &filter_proper_extensions_of(&other.state.acc_places(), &moved_paths) {
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
            for acc_place in self
                .state
                .acc_places()
                .difference(&other.state.acc_places())
            {
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
            for acc_place in other
                .state
                .acc_places()
                .difference(&self.state.acc_places())
            {
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
            for acc_place in self.state.acc_places() {
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
            for pred_place in self.state.pred_places() {
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

            assert_eq!(self.state.acc(), other.state.acc());
            assert_eq!(self.state.pred(), other.state.pred());
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

        let mut actions: Vec<Action> = vec![];

        // info!("Acc state before: {{\n{}\n}}", self.state.display_acc());
        // info!("Pred state before: {{\n{}\n}}", self.state.display_pred());

        // 1. Check if the requirement is satisfied
        if self.state.contains_perm(req) {
            info!("[exit] obtain: Requirement {} is satisfied", req);
            return ObtainResult::Success(actions);
        }

        if req.is_acc() && req.is_local() {
            // access permissions on local variables are always satisfied
            info!("[exit] obtain: Requirement {} is satisfied", req);
            return ObtainResult::Success(actions);
        }

        info!("Try to satisfy requirement {}", req);

        // 3. Obtain with an unfold
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
            let action = self.unfold(&existing_pred_to_unfold, perm_amount, variant);
            actions.push(action);
            info!("We unfolded {}", existing_pred_to_unfold);

            // Check if we are done
            let new_actions = self.obtain(req, false).or_else(|_| ObtainResult::Failure(req.clone()))?;
            actions.extend(new_actions);
            info!("[exit] obtain");
            return ObtainResult::Success(actions);
        }

        // 4. Obtain with a fold
        if req.is_pred() {
            // We want to fold `req`
            info!("We want to fold {}", req);
            info!("We have: acc state: {{\n{}\n}}", self.state.display_acc());
            info!("We have: pred state: {{\n{}\n}}", self.state.display_pred());
            info!("We have: cond state: {{\n{}\n}}", self.state.display_cond());
            let predicate_name = req.typed_ref_name().unwrap();
            let predicate = self.predicates.get(&predicate_name).unwrap();

            let variant = self.find_fold_variant(req);

            // Find an access permission for which req is a proper suffix
            let existing_proper_perm_extension_opt: Option<_> = self
                .state
                .acc_places()
                .into_iter()
                .find(|p| p.has_proper_prefix(req.get_place()));
            info!("predicate is {}", predicate);
            info!("existing_proper_perm_extension_opt is {:?}", existing_proper_perm_extension_opt);
            let pred_self_place: vir::Expr = predicate.self_place();
            info!("pred_self_place is {}", pred_self_place);
            let places_in_pred: Vec<Perm> = predicate
                .get_available_permissions_with_variant(&variant)
                .into_iter()
                .map(|perm| perm.map_place(|p| p.replace_place(&pred_self_place, req.get_place())))
                .filter_map(|ap| match ap {
                    AvailablePerm::Perm(p) => {
                        info!("Simple perm {}", p);
                        Some(p)
                    },
                    // TODO: comment below is wrong
                    // `CondResourceAccess` is always an implication (==>)
                    // Since the implication is always true, we always have that "permission"
                    // so we filter it out.
                    // TODO: for things in acc, we must verify that they verify the cond. Maybe let Viper do that with fold
                    AvailablePerm::Cond(a) => {
                        match &a.resource {
                            ResourceAccess::PredicateAccessPredicate(pred) => {
                                info!("Resource {} {}", pred.predicate_name, pred.arg);
                                for (pred, p) in self.state.acc().clone() {
                                    info!("Pred {}", pred);

                                    // TODO: deal with required prefixes
                                    match a.try_instantiate(&pred) { // TODO: pred? it's acc!
                                        Some(ResourceAccessResult::Predicate {requirements, predicate}) => {
                                            let req = Perm::Pred(*predicate.arg.clone(), p);
                                            info!("REQ {}", req);
                                            info!("PRED {} {}", predicate.predicate_name, predicate.arg);
                                            return Some(req);
                                        }
                                        // TODO: Rest !!
                                        _ => (),
                                    }
                                }
                                None
                            }
                            ResourceAccess::FieldAccessPredicate(f) => {
                                // TODO: for things satisfying the condition, try to obtain the acc of the field in question
                                None
                            }
                        }
                    }
                })
                .collect();
            info!("PLACE IN PRED BEGIN");
            for place in &places_in_pred {
                info!("   {}", place)
            }
            info!("PLACE IN PRED END");
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
                    let obtain_result = self.obtain(&new_req_place, false);
                    match obtain_result {
                        ObtainResult::Success(new_actions) => {
                            actions.extend(new_actions);
                        }
                        ObtainResult::Failure(ref x) => {
                            return obtain_result;
                        }
                    }
                }

                let scaled_places_in_pred: Vec<_> = places_in_pred
                    .into_iter()
                    .map(|perm| perm.update_perm_amount(perm_amount))
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
                // TODO: This can fail. Fix
                /*assert!(
                    !req.get_place().is_simple_place() || self.state.contains_acc(req.get_place()),
                    "req={} state={}",
                    req.get_place(),
                    self.state
                );*/
                assert!(!self.state.contains_pred(req.get_place()));
                self.state.remove_all_perms(scaled_places_in_pred.iter());
                self.state.insert_pred(req.get_place().clone(), perm_amount);

                // Done. Continue checking the remaining requirements
                debug!("We folded {}", req);
                trace!("[exit] obtain");
                return ObtainResult::Success(actions);
            } else {
                info!("Cannot fold; trying contains_cond_perm");
                let result = match self.state.contains_cond_perm(req) {
                    ContainsCondPerm::Yes =>
                        unreachable!("This case should have been covered earlier"),
                    ContainsCondPerm::Partially(mut access_results) => {
                        access_results.retain(ResourceAccessResult::is_predicate);
                        match self.handle_resource_access_results(req, access_results) {
                            ObtainResult::Success(new_actions) => {
                                actions.extend(new_actions);
                                ObtainResult::Success(actions)
                            }
                            ObtainResult::Failure(_) =>
                                ObtainResult::Failure(req.clone())
                        }
                    }
                    ContainsCondPerm::No =>
                        ObtainResult::Failure(req.clone())
                };
                if !result.is_success() {
                    info!(
                        r"It is not possible to obtain {} ({:?}).
Access permissions: {{
{}
}}

Predicates: {{
{}
}}
",
                        req,
                        req,
                        self.state.display_acc(),
                        self.state.display_pred()
                    );
                }
                info!("[exit] obtain: Requirement {} satisfied", req);
                return result;
            }
        } else if in_join && req.get_perm_amount() == vir::PermAmount::Read {
            // Permissions held by shared references can be dropped
            // without being explicitly moved becauce &T implements Copy.
            return ObtainResult::Failure(req.clone());
        } else {
            return match self.state.contains_cond_perm(req) {
                ContainsCondPerm::Yes =>
                    unreachable!("This case should have been covered earlier"),
                ContainsCondPerm::Partially(access_results) => {
                    // info!("Pred state: {{\n{}\n}}", self.state.display_pred());
                    actions.extend(self.handle_resource_access_results(req, access_results)?);
                    ObtainResult::Success(actions)
                }
                ContainsCondPerm::No => {
                    // We have no predicate to obtain the access permission `req`
                    info!(
                        r"There is no access permission to obtain {} ({:?}).
Access permissions: {{
{}
}}

Predicates: {{
{}
}}

Conditional permission: {{
{}
}}
",
                        req,
                        req,
                        self.state.display_acc(),
                        self.state.display_pred(),
                        self.state.display_cond(),
                    );
                    ObtainResult::Failure(req.clone())
                }
            }
        };
    }

    fn handle_resource_access_results(&mut self, req: &Perm, mut access_results: Vec<ResourceAccessResult>) -> ObtainResult {
        fn to_ordinal(ressource_access: &ResourceAccessResult) -> usize {
            match ressource_access {
                ResourceAccessResult::Complete {..} => 0,
                ResourceAccessResult::Predicate {..} => 1,
                ResourceAccessResult::FieldAccessPrefixOnly {..} => 2,
            }
        }

        access_results.sort_by(|lhs, rhs| to_ordinal(lhs).cmp(&to_ordinal(rhs)));
        access_results.into_iter()
            .map(|res| self.handle_resource_access_result(req, res))
            .find(|obtain_res| obtain_res.is_success())
            .unwrap_or_else(|| ObtainResult::Failure(req.clone()))
    }

    fn handle_resource_access_result(&mut self, req: &Perm, access_result: ResourceAccessResult) -> ObtainResult {
        let mut actions = Vec::new();
        match access_result {
            ResourceAccessResult::Complete { requirements } => {
                // TODO: We could self.obtain over the requirements:
                //  -If we succeed, good
                //  -Otherwise, we assert the requirements
                info!("[exit] obtain: Requirement {} is satisfied only if {} is satisfied", req, requirements);
                // TODO: pull out this
                actions.push(Action::Assertion(requirements));
                // TODO: insert "assertions" ?
                self.state.insert_perm(req.clone());
                ObtainResult::Success(actions)
            }
            ResourceAccessResult::Predicate { requirements, predicate } => {
                info!("obtain: Required permission {} may be satisfied with predicate {}", req, predicate.predicate_name);
                actions.push(Action::Assertion(requirements));
                if req.is_pred() {
                    if &*predicate.arg == req.get_place() {
                        // TODO: nothing else to do?
                        self.state.insert_pred(req.get_place().clone(), req.get_perm_amount());
                        ObtainResult::Success(actions)
                    } else {
                        // TODO: this is too harsh. Maybe try to obtain acc on prefix
                        ObtainResult::Failure(req.clone())
                    }
                } else {
                    // let predicate_perm = Perm::Pred(*predicate.arg, predicate.perm); // TODO: Predicate body or argument?
                    // actions.extend(self.obtain(&predicate_perm, false).unwrap());
                    self.state.insert_pred(*predicate.arg.clone(), predicate.perm);
                    actions.push(self.unfold(&*predicate.arg, predicate.perm, None));
                    // self.state.insert_perm(req.clone());
                    info!("[exit] obtain: Required permission {} satisfied", req);
                    ObtainResult::Success(actions)
                }
            }
            ResourceAccessResult::FieldAccessPrefixOnly { requirements, prefix } => {
                info!("REQUEST {}    prefix {}", req, prefix.place);
                // TODO: get the prefix then retry
                unimplemented!()
                /*let prefix_perm = Perm::Acc(*prefix.place, prefix.perm);
                let new_actions = self.obtain(&prefix_perm, false).unwrap();
                info!("[exit] obtain: Requirement {} is satisfied only if {} is satisfied", req, requirements);
                actions.extend(new_actions);
                // TODO: pull out this
                actions.push(Action::Assertion(requirements));
                self.state.insert_perm(req.clone());
                ObtainResult::Success(actions)*/
            }
        }
    }

    /// Returns some of the dropped permissions
    pub fn apply_stmt(&mut self, stmt: &vir::Stmt) {
        info!("apply_stmt: {}", stmt);

        // info!("Acc state before: {{\n{}\n}}", self.state.display_acc());
        // info!("Pred state before: {{\n{}\n}}", self.state.display_pred());
        // info!("Cond state before: {{\n{}\n}}", self.state.display_cond());

        self.state.check_consistency();

        stmt.apply_on_state(&mut self.state, self.predicates);

        // info!("Acc state after: {{\n{}\n}}", self.state.display_acc());
        // info!("Pred state after: {{\n{}\n}}", self.state.display_pred());
        // info!("Cond state after: {{\n{}\n}}", self.state.display_cond());

        self.state.check_consistency();
    }

    pub fn obtain_permissions(&mut self, permissions: Vec<Perm>) -> Vec<Action> {
        // info!(
        //     "[enter] obtain_permissions: {}",
        //     permissions.iter().to_string()
        // );

        // info!("Acc state before: {{\n{}\n}}", self.state.display_acc());
        // info!("Pred state before: {{\n{}\n}}", self.state.display_pred());
        // info!("Cond state before: {{\n{}\n}}", self.state.display_cond());

        self.state.check_consistency();

        let actions = self.obtain_all(permissions);

        // info!("Acc state after: {{\n{}\n}}", self.state.display_acc());
        // info!("Pred state after: {{\n{}\n}}", self.state.display_pred());
        // info!("Cond state after: {{\n{}\n}}", self.state.display_cond());

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