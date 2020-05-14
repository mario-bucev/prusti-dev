// © 2019, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::places_utils::{union, union3};
use encoder::foldunfold::perm::Perm::*;
use encoder::foldunfold::perm::*;
use encoder::vir;
use encoder::vir::PermAmount;
use std::collections::HashMap;
use std::collections::HashSet;
use std::iter::FromIterator;

pub trait RequiredPermissionsGetter {
    /// Returns the permissions required for the expression to be well-defined
    fn get_required_permissions(
        &self,
        predicates: &HashMap<String, vir::Predicate>,
    ) -> HashSet<Perm>;
}

impl<'a, A: RequiredPermissionsGetter> RequiredPermissionsGetter for Vec<&'a A> {
    /// Returns the permissions required for the expression to be well-defined
    fn get_required_permissions(
        &self,
        predicates: &HashMap<String, vir::Predicate>,
    ) -> HashSet<Perm> {
        self.iter().fold(HashSet::new(), |res, x| {
            res.union(&x.get_required_permissions(predicates))
                .cloned()
                .collect()
        })
    }
}

impl RequiredPermissionsGetter for Vec<vir::Expr> {
    /// Returns the permissions required for the expression to be well-defined
    fn get_required_permissions(
        &self,
        predicates: &HashMap<String, vir::Predicate>,
    ) -> HashSet<Perm> {
        self.iter().fold(HashSet::new(), |res, x| {
            res.union(&x.get_required_permissions(predicates))
                .cloned()
                .collect()
        })
    }
}

impl RequiredPermissionsGetter for vir::Stmt {
    /// Returns the permissions required for the statement to be well-defined
    fn get_required_permissions(
        &self,
        predicates: &HashMap<String, vir::Predicate>,
    ) -> HashSet<Perm> {
        match self {
            &vir::Stmt::Comment(_) | &vir::Stmt::Label(_) => HashSet::new(),

            &vir::Stmt::Inhale(ref expr, _) => {
                // footprint = used - inhaled
                perm_difference(
                    expr.get_required_permissions(predicates),
                    expr.get_permissions(predicates),
                )
            }

            &vir::Stmt::Exhale(ref expr, ref pos)
            | &vir::Stmt::Assert(ref expr, _, ref pos)
            | &vir::Stmt::Obtain(ref expr, ref pos) => {
                let perms = expr.get_required_permissions(predicates);
                perms
                    .into_iter()
                    .map(|perm| perm.set_default_pos(pos.clone()))
                    .collect()
            }

            &vir::Stmt::MethodCall(_, ref args, ref vars) => {
                // Preconditions and postconditions are empty
                assert!(args.is_empty());
                HashSet::from_iter(
                    vars.iter()
                        .cloned()
                        .map(|v| Acc(vir::Expr::local(v), PermAmount::Write)),
                )
            }

            &vir::Stmt::Assign(ref lhs, ref rhs, _kind) => {
                let mut res = rhs.get_required_permissions(predicates);
                res.insert(Acc(lhs.clone(), PermAmount::Write));
                res
            }

            &vir::Stmt::Fold(_, ref args, perm_amount, ref variant, _) => {
                assert_eq!(args.len(), 1);
                let place = &args[0];
                debug_assert!(place.is_place());

                // We want to temporarly unfold place
                let predicate_name = place.typed_ref_name().unwrap();
                let predicate = predicates.get(&predicate_name).unwrap();

                let pred_self_place: vir::Expr = predicate.self_place();
                let places_in_pred: HashSet<Perm> = predicate
                    .get_permissions_with_variant(variant)
                    .into_iter()
                    .map(|perm| {
                        perm.map_place(|p| p.replace_place(&pred_self_place, &place))
                            .init_perm_amount(perm_amount)
                    })
                    .collect();

                places_in_pred
            }

            &vir::Stmt::Unfold(ref _pred_name, ref args, perm_amount, ref _variant) => {
                assert_eq!(args.len(), 1);
                let place = &args[0];
                debug_assert!(place.is_place());
                place
                    .get_required_permissions(predicates)
                    .into_iter()
                    .map(|perm| perm.init_perm_amount(perm_amount))
                    .collect()
            }

            &vir::Stmt::BeginFrame | &vir::Stmt::EndFrame => HashSet::new(),

            &vir::Stmt::TransferPerm(ref lhs, _, unchecked) => {
                let mut res = HashSet::new();
                if !unchecked {
                    res.insert(Acc(lhs.clone(), PermAmount::Read));
                }
                res
            }

            &vir::Stmt::PackageMagicWand(
                vir::Expr::MagicWand(ref _lhs, ref _rhs, ref _borrow, ref _pos),
                ref _package_stmts,
                ref _label,
                ref _vars,
                ref _position,
            ) => {
                // We model the magic wand as "assert lhs; stmts; exhale rhs"
                HashSet::new()
            }

            &vir::Stmt::ApplyMagicWand(
                vir::Expr::MagicWand(ref lhs, ref _rhs, ref _borrow, ref _wand_pos),
                ref _apply_pos,
            ) => {
                // We model the magic wand as "assert lhs; inhale rhs"
                lhs.get_required_permissions(predicates)
            }

            &vir::Stmt::ExpireBorrows(ref _dag) => {
                HashSet::new() // TODO: #133
            }

            ref x => unimplemented!("{}", x),
        }
    }
}

impl RequiredPermissionsGetter for vir::Expr {
    /// Returns the permissions required for the expression to be well-defined
    fn get_required_permissions(
        &self,
        predicates: &HashMap<String, vir::Predicate>,
    ) -> HashSet<Perm> {
        info!("[enter] get_required_permissions(expr={})", self);
        let permissions = match self {
            vir::Expr::Const(_, _) => HashSet::new(),

            vir::Expr::Unfolding(_, args, expr, perm_amount, variant, _) => {
                assert_eq!(args.len(), 1);
                let place = &args[0];
                debug_assert!(place.is_place());

                // We want to temporarly unfold place
                let predicate_name = place.typed_ref_name().unwrap();
                let predicate = predicates.get(&predicate_name).unwrap();

                let pred_self_place: vir::Expr = predicate.self_place();
                let places_in_pred: HashSet<Perm> = predicate
                    .get_permissions_with_variant(variant)
                    .into_iter()
                    .map(|aop| {
                        aop.map_place(|p| p.replace_place(&pred_self_place, place))
                            .update_perm_amount(*perm_amount)
                    })
                    .collect();

                // Simulate temporary unfolding of `place`
                let expr_req_places = expr.get_required_permissions(predicates);
                let mut req_places: HashSet<_> = perm_difference(expr_req_places, places_in_pred);
                req_places.insert(Pred(place.clone(), *perm_amount));
                req_places.into_iter().collect()
            }

            vir::Expr::LabelledOld(_label, _expr, _) => HashSet::new(),

            vir::Expr::PredicateAccessPredicate(_, box place, _perm_amount, _) => {
                debug_assert!(place.is_place());
                let epsilon = PermAmount::Read;
                let result = match place.get_label() {
                    None => {
                        if place.is_old() {
                            vec![Pred(place.clone(), epsilon)].into_iter().collect()
                        } else {
                            vec![Pred(place.clone(), epsilon), Acc(place.clone(), epsilon)]
                                .into_iter()
                                .collect()
                        }
                    }
                    Some(label) => {
                        if place.is_old() {
                            vec![Pred(place.clone().old(label), epsilon)]
                                .into_iter()
                                .collect()
                        } else {
                            vec![
                                Pred(place.clone().old(label), epsilon),
                                Acc(place.clone().old(label), epsilon),
                            ]
                            .into_iter()
                            .collect()
                        }
                    }
                };
                result
            }

            vir::Expr::FieldAccessPredicate(expr, _perm_amount, _) => expr
                .get_required_permissions(predicates)
                .into_iter()
                .collect(),

            vir::Expr::UnaryOp(_, expr, _) => expr.get_required_permissions(predicates),

            vir::Expr::BinOp(_, box left, box right, _) => {
                vec![left, right].get_required_permissions(predicates)
            }

            vir::Expr::Cond(box guard, box left, box right, _) => {
                vec![guard, left, right].get_required_permissions(predicates)
            }

            vir::Expr::LetExpr(_variable, _expr, _body, _) => {
                unreachable!("Let expressions should be introduced after fold/unfold.");
            }

            // FIXME: if there are subsequent implications in `body`, they will not be translated correctly !
            //  e.g. forall vars :: cond1 ==> cond2 ==> expr will give back
            //  QuantifiedPermissions(cond1, permissions(cond2) ++ permissions(expr))
            //  The cond2 disappears
            // If we have an expr of the form `forall vars :: cond ==> expr`
            // we convert the required permission into a quantified permission.
            vir::Expr::ForAll(
                vars,
                triggers,
                box vir::Expr::BinOp(vir::BinOpKind::Implies, box cond, box body, _),
                _
            ) => {
                let vars_places: HashSet<_> = vars
                    .iter()
                    .map(|var| Acc(vir::Expr::local(var.clone()), PermAmount::Write))
                    .collect();
                let vars_set = vars.iter().cloned().collect();
                let mut perm_body: HashSet<_> = perm_difference(
                    body.get_required_permissions(predicates),
                    vars_places.clone()
                ).into_iter().map(|perm| {
                    let vars_in_perm = perm.get_place().get_local_vars(&vars_set);
                    if vars_in_perm.is_empty() {
                        perm
                    } else {
                        let plain_perm = match perm {
                            Acc(place, amount) => vir::PlainResourceAccess::Field(
                                vir::FieldAccessPredicate {
                                    place: box place,
                                    perm: amount
                                }
                            ),
                            Pred(place, amount) => vir::PlainResourceAccess::Predicate(
                                vir::PredicateAccessPredicate {
                                    predicate_name: place.typed_ref_name().unwrap(),
                                    arg: box place,
                                    perm: amount
                                }
                            ),
                            Quantified(_) => unimplemented!(),
                        };
                        let filtered_triggers = triggers.iter()
                            .map(|trigger|
                                vir::Trigger::new(trigger.elements()
                                    .iter()
                                    .filter(|e|
                                        e.get_local_vars(&vars_in_perm).len() == vars_in_perm.len()
                                    ).cloned()
                                    .collect()
                                )
                            ).collect();

                        Perm::Quantified(
                            vir::QuantifiedResourceAccess {
                                vars: vars_in_perm.into_iter().collect(),
                                triggers: filtered_triggers,
                                cond: box cond.clone(),
                                resource: plain_perm,
                            }
                        )
                    }
                }).collect();
                let perm_cond =
                    perm_difference(cond.get_required_permissions(predicates), vars_places);
                perm_body.extend(perm_cond);
                perm_body
            }

            vir::Expr::ForAll(vars, _triggers, box body, _) => {
                assert!(vars.iter().all(|var| !var.typ.is_ref()));

                let vars_places: HashSet<_> = vars
                    .iter()
                    .map(|var| Acc(vir::Expr::local(var.clone()), PermAmount::Write))
                    .collect();
                perm_difference(body.get_required_permissions(predicates), vars_places)
            }

            vir::Expr::Local(..) => HashSet::new(),

            vir::Expr::AddrOf(..) => unreachable!(),

            vir::Expr::Variant(..) => Some(Acc(self.clone(), PermAmount::Read))
                .into_iter()
                .collect(),

            vir::Expr::Field(..) => Some(Acc(self.clone(), PermAmount::Read))
                .into_iter()
                .collect(),

            vir::Expr::MagicWand(ref _lhs, ref _rhs, ref _borrow, _) => {
                // Not exactly Viper's semantics
                HashSet::new()
            }

            vir::Expr::FuncApp(ref _name, ref args, ..) => {
                args.iter()
                    .map(|arg| {
                        if arg.is_place() && arg.get_type().is_ref() {
                            // FIXME: A hack: have unfolded Rust references in the precondition to
                            // simplify our life. A proper solution would be to look up the
                            // real function precondition.
                            if let Some(field_place) = arg.try_deref() {
                                vir::Expr::and(
                                    vir::Expr::acc_permission(
                                        field_place.clone(),
                                        vir::PermAmount::Read,
                                    ),
                                    vir::Expr::pred_permission(field_place, vir::PermAmount::Read)
                                        .unwrap(),
                                )
                            } else {
                                let predicate_name = arg.get_type().name();
                                vir::Expr::predicate_access_predicate(
                                    predicate_name.clone(),
                                    arg.clone().into(),
                                    PermAmount::Read,
                                )
                            }
                        } else {
                            debug!("arg {} is not a place with type ref", arg);
                            arg.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .get_required_permissions(predicates)
            }

            vir::Expr::SeqIndex(box seq, box index, _) =>
                vec![seq, index].get_required_permissions(predicates),

            vir::Expr::SeqLen(ref seq, _) =>
                seq.get_required_permissions(predicates),

            vir::Expr::QuantifiedResourceAccess(quant, _) =>
                Some(Quantified(quant.clone().update_perm_amount(PermAmount::Read)))
                    .into_iter()
                    .collect(),
        };
        info!(
            "[exit] get_required_permissions(expr={}):\n{}",
            self,
            permissions
                .iter()
                .map(|p| format!("  {}", p))
                .collect::<Vec<String>>()
                .join(",\n")
        );
        permissions
    }
}

impl vir::Expr {
    /// Returns the permissions that must be inhaled/exhaled in a `inhale/exhale expr` statement
    /// This must be a subset of `get_required_permissions`
    pub fn get_permissions(&self, predicates: &HashMap<String, vir::Predicate>) -> HashSet<Perm> {
        trace!("get_permissions {}", self);
        match self {
            vir::Expr::Local(_, _)
            | vir::Expr::Field(_, _, _)
            | vir::Expr::Variant(_, _, _)
            | vir::Expr::AddrOf(_, _, _)
            | vir::Expr::LabelledOld(_, _, _)
            | vir::Expr::Const(_, _)
            | vir::Expr::FuncApp(..)
            | vir::Expr::SeqIndex(_, _, _)
            | vir::Expr::SeqLen(_, _) => HashSet::new(),

            vir::Expr::Unfolding(_, args, expr, perm_amount, variant, _) => {
                assert_eq!(args.len(), 1);
                let place = &args[0];
                debug_assert!(place.is_place());

                // We want to temporarly unfold place
                let predicate_name = place.typed_ref_name().unwrap();
                let predicate = predicates.get(&predicate_name).unwrap();

                let pred_self_place: vir::Expr = predicate.self_place();
                let places_in_pred: HashSet<Perm> = predicate
                    .get_permissions_with_variant(variant)
                    .into_iter()
                    .map(|aop| {
                        aop.map_place(|p| p.replace_place(&pred_self_place, place))
                            .init_perm_amount(*perm_amount)
                    })
                    .collect();

                // Simulate temporary unfolding of `place`
                let expr_access_places = expr.get_permissions(predicates);

                // inhaled = inhaled in body - unfolding
                perm_difference(expr_access_places, places_in_pred)
            }

            vir::Expr::UnaryOp(_, ref expr, _) => expr.get_permissions(predicates),

            vir::Expr::BinOp(_, box left, box right, _) => union(
                &left.get_permissions(predicates),
                &right.get_permissions(predicates),
            ),

            vir::Expr::Cond(box guard, box left, box right, _) => union3(
                &guard.get_permissions(predicates),
                &left.get_permissions(predicates),
                &right.get_permissions(predicates),
            ),

            vir::Expr::ForAll(vars, _triggers, box body, _) => {
                assert!(vars.iter().all(|var| !var.typ.is_ref()));
                let vars_places: HashSet<Perm> = vars
                    .iter()
                    .map(|var| Acc(vir::Expr::local(var.clone()), PermAmount::Write))
                    .collect();
                perm_difference(body.get_permissions(predicates), vars_places)
            }

            vir::Expr::PredicateAccessPredicate(_, box ref arg, perm_amount, _) => {
                let opt_perm = if arg.is_place() {
                    Some(match arg.get_label() {
                        None => Perm::Pred(arg.clone(), *perm_amount),
                        Some(label) => Perm::Pred(arg.clone().old(label), *perm_amount),
                    })
                } else {
                    None
                };

                opt_perm.into_iter().collect()
            }

            vir::Expr::FieldAccessPredicate(box ref place, perm_amount, _) => {
                // In Prusti we assume to have only places here
                debug_assert!(place.is_place());
                debug_assert!(place.is_curr());

                let perm = Acc(place.clone(), *perm_amount);

                Some(perm).into_iter().collect()
            }

            vir::Expr::MagicWand(ref _lhs, ref _rhs, _borrow, _) => {
                // We don't track magic wands resources
                HashSet::new()
            }

            vir::Expr::LetExpr(ref _variable, ref _expr, ref _body, _) => {
                unreachable!("Let expressions should be introduced after fold/unfold.");
            }

            vir::Expr::QuantifiedResourceAccess(ref quant, _) =>
                Some(Quantified(quant.clone()))
                    .into_iter()
                    .collect(),
        }
    }
}

impl vir::Predicate {
    /// Returns the permissions that must be added/removed in a `fold/unfold pred` statement
    pub fn get_permissions_with_variant(
        &self,
        maybe_variant: &vir::MaybeEnumVariantIndex
    ) -> HashSet<Perm> {
        let perms = match self {
            vir::Predicate::Struct(p) => {
                assert!(maybe_variant.is_none());
                p.get_permissions()
            },
            vir::Predicate::Enum(p) => {
                if let Some(variant) = maybe_variant {
                    p.get_permissions(variant)
                } else {
                    // We must be doing fold/unfold for a pure function.
                    p.get_all_permissions()
                }
            },
        };
        perms
    }
}

impl vir::StructPredicate {
    /// Returns the permissions that must be added/removed in a `fold/unfold pred` statement
    pub fn get_permissions(&self) -> HashSet<Perm> {
        match self.body {
            Some(ref body) => {
                // A predicate body should not contain unfolding expression
                let predicates = HashMap::new();
                body.get_permissions(&predicates)
            }
            None => HashSet::new(),
        }
    }
}

impl vir::EnumPredicate {
    /// Returns the permissions that must be added/removed in a `fold/unfold pred` statement
    pub fn get_permissions(&self, variant: &vir::EnumVariantIndex) -> HashSet<Perm> {
        // A predicate body should not contain unfolding expression
        let predicates = HashMap::new();
        let mut perms = self.discriminant.get_required_permissions(&predicates);
        let this: vir::Expr = self.this.clone().into();
        //let (_, ref variant_name, _) = &self.variants[variant];
        let variant_name = variant.get_variant_name();
        perms.insert(
            Perm::Acc(
                this.clone().variant(variant_name),
                PermAmount::Write,
            )
        );
        perms.insert(
            Perm::Pred(
                this.clone().variant(variant_name),
                PermAmount::Write,
            )
        );
        perms
    }
    /// Returns the permissions that must be added/removed in a `fold/unfold pred` statement
    pub fn get_all_permissions(&self) -> HashSet<Perm> {
        // A predicate body should not contain unfolding expression
        let predicates = HashMap::new();
        let mut perms = self.discriminant.get_required_permissions(&predicates);
        let this: vir::Expr = self.this.clone().into();
        for (_guard, variant_name, _variant_predicate) in &self.variants {
            perms.insert(Perm::Acc(
                this.clone().variant(&variant_name),
                PermAmount::Write,
            ));
            perms.insert(Perm::Pred(
                this.clone().variant(&variant_name),
                PermAmount::Write,
            ));
        }
        perms
    }
}
