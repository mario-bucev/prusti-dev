// © 2019, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::super::borrows::Borrow;
use encoder::vir::ast::*;
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::mem::discriminant;

#[derive(Debug, Clone)]
pub enum Expr {
    /// A local var
    Local(LocalVar, Position),
    /// An enum variant: base, variant index.
    Variant(Box<Expr>, Field, Position),
    /// A field access
    Field(Box<Expr>, Field, Position),
    /// The inverse of a `val_ref` field access
    AddrOf(Box<Expr>, Type, Position),
    LabelledOld(String, Box<Expr>, Position),
    Const(Const, Position),
    /// lhs, rhs, borrow, position
    MagicWand(Box<Expr>, Box<Expr>, Option<Borrow>, Position),
    /// PredicateAccessPredicate: predicate_name, arg, permission amount
    PredicateAccessPredicate(String, Box<Expr>, PermAmount, Position),
    FieldAccessPredicate(Box<Expr>, PermAmount, Position),
    QuantifiedResourceAccess(QuantifiedResourceAccess, Position),
    UnaryOp(UnaryOpKind, Box<Expr>, Position),
    BinOp(BinOpKind, Box<Expr>, Box<Expr>, Position),
    /// Unfolding: predicate name, predicate_args, in_expr, permission amount, enum variant
    Unfolding(String, Vec<Expr>, Box<Expr>, PermAmount, MaybeEnumVariantIndex, Position),
    /// Cond: guard, then_expr, else_expr
    Cond(Box<Expr>, Box<Expr>, Box<Expr>, Position),
    /// ForAll: variables, triggers, body
    ForAll(Vec<LocalVar>, Vec<Trigger>, Box<Expr>, Position),
    /// let variable == (expr) in body
    LetExpr(LocalVar, Box<Expr>, Box<Expr>, Position),
    /// FuncApp: function_name, args, formal_args, return_type, Viper position
    FuncApp(String, Vec<Expr>, Vec<LocalVar>, Type, Position),
    /// An indexing into a Seq: sequence, index, position
    /// Important note: A SeqIndex expression must always be "contained" in a field projection
    /// of `val_ref`. That is, we must always have something of the form `seq[idx].val_ref`
    /// Otherwise, things like assignment into the sequence won't work
    SeqIndex(Box<Expr>, Box<Expr>, Position),
    /// Length of the given sequence
    SeqLen(Box<Expr>, Position),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlainResourceAccess {
    Predicate(PredicateAccessPredicate),
    Field(FieldAccessPredicate)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PredicateAccessPredicate {
    pub predicate_name: String,
    pub arg: Box<Expr>,
    pub perm: PermAmount
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldAccessPredicate {
    pub place: Box<Expr>,
    pub perm: PermAmount
}

/// A quantified resource access.
///
/// This is a more specified version of the following expression:
/// `forall vars :: { triggers } cond ==> resource`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QuantifiedResourceAccess {
    pub vars: Vec<LocalVar>,
    pub triggers: Vec<Trigger>,
    pub cond: Box<Expr>,
    pub resource: PlainResourceAccess,
}

/// A component that can be used to represent a place as a vector.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlaceComponent {
    Field(Field, Position),
    Variant(Field, Position),
    SeqIndex(Box<Expr>, Position),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOpKind {
    Not,
    Minus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOpKind {
    EqCmp,
    NeCmp,
    GtCmp,
    GeCmp,
    LtCmp,
    LeCmp,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Implies,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Const {
    Bool(bool),
    Int(i64),
    BigInt(String),
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Expr::Local(ref v, ref _pos) => write!(f, "{}", v),
            Expr::Variant(ref base, ref variant_index, ref _pos) => {
                write!(f, "{}[{}]", base, variant_index)
            }
            Expr::Field(ref base, ref field, ref _pos) => write!(f, "{}.{}", base, field),
            Expr::AddrOf(ref base, _, ref _pos) => write!(f, "&({})", base),
            Expr::Const(ref value, ref _pos) => write!(f, "{}", value),
            Expr::BinOp(op, ref left, ref right, ref _pos) => {
                write!(f, "({}) {} ({})", left, op, right)
            }
            Expr::UnaryOp(op, ref expr, ref _pos) => write!(f, "{}({})", op, expr),
            Expr::PredicateAccessPredicate(ref pred_name, ref arg, perm, ref _pos) => {
                write!(f, "acc({}({}), {})", pred_name, arg, perm)
            }
            Expr::FieldAccessPredicate(ref expr, perm, ref _pos) => {
                write!(f, "acc({}, {})", expr, perm)
            }
            Expr::LabelledOld(ref label, ref expr, ref _pos) => {
                write!(f, "old[{}]({})", label, expr)
            }
            Expr::MagicWand(ref left, ref right, ref borrow, ref _pos) => {
                write!(f, "({}) {:?} --* ({})", left, borrow, right)
            }
            Expr::Unfolding(ref pred_name, ref args, ref expr, perm, ref variant, ref _pos) => {
                write!(
                    f,
                    "(unfolding acc({}:{:?}({}), {}) in {})",
                    pred_name,
                    variant,
                    args.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<String>>()
                        .join(", "),
                    perm,
                    expr
                )
            },
            Expr::Cond(ref guard, ref left, ref right, ref _pos) => {
                write!(f, "({})?({}):({})", guard, left, right)
            }
            Expr::ForAll(ref vars, ref triggers, ref body, ref _pos) => write!(
                f,
                "forall {} {} :: {}",
                vars.iter()
                    .map(|x| format!("{:?}", x))
                    .collect::<Vec<String>>()
                    .join(", "),
                triggers
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<String>>()
                    .join(", "),
                body.to_string()
            ),
            Expr::LetExpr(ref var, ref expr, ref body, ref _pos) => write!(
                f,
                "(let {:?} == ({}) in {})",
                var,
                expr.to_string(),
                body.to_string()
            ),
            Expr::FuncApp(ref name, ref args, ref params, ref typ, ref _pos) => write!(
                f,
                "{}<{},{}>({})",
                name,
                params
                    .iter()
                    .map(|p| p.typ.to_string())
                    .collect::<Vec<String>>()
                    .join(", "),
                typ.to_string(),
                args.iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<String>>()
                    .join(", "),
            ),
            Expr::SeqIndex(ref seq, ref index, _) => write!(f, "{}[{}]", seq, index),
            Expr::SeqLen(ref seq, _) => write!(f, "|{}|", seq),
            Expr::QuantifiedResourceAccess(ref quant, _) => quant.fmt(f),
        }
    }
}

impl fmt::Display for UnaryOpKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &UnaryOpKind::Not => write!(f, "!"),
            &UnaryOpKind::Minus => write!(f, "-"),
        }
    }
}

impl fmt::Display for BinOpKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &BinOpKind::EqCmp => write!(f, "=="),
            &BinOpKind::NeCmp => write!(f, "!="),
            &BinOpKind::GtCmp => write!(f, ">"),
            &BinOpKind::GeCmp => write!(f, ">="),
            &BinOpKind::LtCmp => write!(f, "<"),
            &BinOpKind::LeCmp => write!(f, "<="),
            &BinOpKind::Add => write!(f, "+"),
            &BinOpKind::Sub => write!(f, "-"),
            &BinOpKind::Mul => write!(f, "*"),
            &BinOpKind::Div => write!(f, "\\"),
            &BinOpKind::Mod => write!(f, "%"),
            &BinOpKind::And => write!(f, "&&"),
            &BinOpKind::Or => write!(f, "||"),
            &BinOpKind::Implies => write!(f, "==>"),
        }
    }
}

impl fmt::Display for Const {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &Const::Bool(val) => write!(f, "{}", val),
            &Const::Int(val) => write!(f, "{}", val),
            &Const::BigInt(ref val) => write!(f, "{}", val),
        }
    }
}

impl fmt::Display for FieldAccessPredicate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "acc({}, {})", self.place, self.perm)
    }
}

impl fmt::Display for PredicateAccessPredicate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "acc({}({}), {})", self.predicate_name, self.arg, self.perm)
    }
}

impl fmt::Display for PlainResourceAccess {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PlainResourceAccess::Field(fa) => fa.fmt(f),
            PlainResourceAccess::Predicate(pa) => pa.fmt(f),
        }
    }
}

impl fmt::Display for QuantifiedResourceAccess {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.vars.is_empty() {
            write!(f, "{} ==> {}", self.cond, self.resource)
        } else {
            write!(
                f,
                "forall {} {} :: {} ==> {}",
                self.vars.iter()
                    .map(|x| format!("{:?}", x))
                    .collect::<Vec<String>>()
                    .join(", "),
                self.triggers
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<String>>()
                    .join(", "),
                self.cond,
                self.resource
            )
        }
    }
}

impl Expr {
    pub fn pos(&self) -> &Position {
        match self {
            Expr::Local(_, ref p) => p,
            Expr::Variant(_, _, ref p) => p,
            Expr::Field(_, _, ref p) => p,
            Expr::AddrOf(_, _, ref p) => p,
            Expr::Const(_, ref p) => p,
            Expr::LabelledOld(_, _, ref p) => p,
            Expr::MagicWand(_, _, _, ref p) => p,
            Expr::PredicateAccessPredicate(_, _, _, ref p) => p,
            Expr::FieldAccessPredicate(_, _, ref p) => p,
            Expr::UnaryOp(_, _, ref p) => p,
            Expr::BinOp(_, _, _, ref p) => p,
            Expr::Unfolding(_, _, _, _, _, ref p) => p,
            Expr::Cond(_, _, _, ref p) => p,
            Expr::ForAll(_, _, _, ref p) => p,
            Expr::LetExpr(_, _, _, ref p) => p,
            Expr::FuncApp(_, _, _, _, ref p) => p,
            Expr::SeqIndex(_, _, ref p) => p,
            Expr::SeqLen(_, ref p) => p,
            Expr::QuantifiedResourceAccess(_, ref p) => p,
        }
    }

    pub fn set_pos(self, pos: Position) -> Self {
        match self {
            Expr::Local(v, _) => Expr::Local(v, pos),
            Expr::Variant(base, variant_index, _) => Expr::Variant(base, variant_index, pos),
            Expr::Field(e, f, _) => Expr::Field(e, f, pos),
            Expr::AddrOf(e, t, _) => Expr::AddrOf(e, t, pos),
            Expr::Const(x, _) => Expr::Const(x, pos),
            Expr::LabelledOld(x, y, _) => Expr::LabelledOld(x, y, pos),
            Expr::MagicWand(x, y, b, _) => Expr::MagicWand(x, y, b, pos),
            Expr::PredicateAccessPredicate(x, y, z, _) => {
                Expr::PredicateAccessPredicate(x, y, z, pos)
            }
            Expr::FieldAccessPredicate(x, y, _) => Expr::FieldAccessPredicate(x, y, pos),
            Expr::UnaryOp(x, y, _) => Expr::UnaryOp(x, y, pos),
            Expr::BinOp(x, y, z, _) => Expr::BinOp(x, y, z, pos),
            Expr::Unfolding(x, y, z, perm, variant, _) => {
                Expr::Unfolding(x, y, z, perm, variant, pos)
            },
            Expr::Cond(x, y, z, _) => Expr::Cond(x, y, z, pos),
            Expr::ForAll(x, y, z, _) => Expr::ForAll(x, y, z, pos),
            Expr::LetExpr(x, y, z, _) => Expr::LetExpr(x, y, z, pos),
            Expr::FuncApp(x, y, z, k, _) => Expr::FuncApp(x, y, z, k, pos),
            Expr::SeqIndex(x, y, _) => Expr::SeqIndex(x, y, pos),
            Expr::SeqLen(x, _) => Expr::SeqLen(x, pos),
            Expr::QuantifiedResourceAccess(x, _) => Expr::QuantifiedResourceAccess(x, pos),
        }
    }

    // Replace all Position::default() positions with `pos`
    pub fn set_default_pos(self, pos: Position) -> Self {
        struct DefaultPosReplacer {
            new_pos: Position,
        };
        impl ExprFolder for DefaultPosReplacer {
            fn fold(&mut self, e: Expr) -> Expr {
                let expr = default_fold_expr(self, e);
                if expr.pos().is_default() {
                    expr.set_pos(self.new_pos.clone())
                } else {
                    expr
                }
            }
        }
        DefaultPosReplacer { new_pos: pos }.fold(self)
    }

    pub fn predicate_access_predicate<S: ToString>(name: S, place: Expr, perm: PermAmount) -> Self {
        let pos = place.pos().clone();
        Expr::PredicateAccessPredicate(name.to_string(), box place, perm, pos)
    }

    pub fn pred_permission(place: Expr, perm: PermAmount) -> Option<Self> {
        place
            .typed_ref_name()
            .map(|pred_name| Expr::predicate_access_predicate(pred_name, place, perm))
    }

    pub fn acc_permission(place: Expr, perm: PermAmount) -> Self {
        Expr::FieldAccessPredicate(box place, perm, Position::default())
    }

    pub fn labelled_old(label: &str, expr: Expr) -> Self {
        Expr::LabelledOld(label.to_string(), box expr, Position::default())
    }

    pub fn not(expr: Expr) -> Self {
        Expr::UnaryOp(UnaryOpKind::Not, box expr, Position::default())
    }

    pub fn minus(expr: Expr) -> Self {
        Expr::UnaryOp(UnaryOpKind::Minus, box expr, Position::default())
    }

    pub fn gt_cmp(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::GtCmp, box left, box right, Position::default())
    }

    pub fn ge_cmp(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::GeCmp, box left, box right, Position::default())
    }

    pub fn lt_cmp(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::LtCmp, box left, box right, Position::default())
    }

    pub fn le_cmp(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::LeCmp, box left, box right, Position::default())
    }

    pub fn eq_cmp(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::EqCmp, box left, box right, Position::default())
    }

    pub fn ne_cmp(left: Expr, right: Expr) -> Self {
        Expr::not(Expr::eq_cmp(left, right))
    }

    pub fn add(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::Add, box left, box right, Position::default())
    }

    pub fn sub(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::Sub, box left, box right, Position::default())
    }

    pub fn mul(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::Mul, box left, box right, Position::default())
    }

    pub fn div(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::Div, box left, box right, Position::default())
    }

    pub fn modulo(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::Mod, box left, box right, Position::default())
    }

    /// Encode Rust reminder. This is *not* Viper modulo.
    pub fn rem(left: Expr, right: Expr) -> Self {
        let abs_right = Expr::ite(
            Expr::ge_cmp(right.clone(), 0.into()),
            right.clone(),
            Expr::minus(right.clone()),
        );
        Expr::ite(
            Expr::or(
                Expr::ge_cmp(left.clone(), 0.into()),
                Expr::eq_cmp(Expr::modulo(left.clone(), right.clone()), 0.into()),
            ),
            // positive value or left % right == 0
            Expr::modulo(left.clone(), right.clone()),
            // negative value
            Expr::sub(Expr::modulo(left, right), abs_right),
        )
    }

    pub fn and(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::And, box left, box right, Position::default())
    }

    pub fn or(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::Or, box left, box right, Position::default())
    }

    pub fn xor(left: Expr, right: Expr) -> Self {
        Expr::not(Expr::eq_cmp(left, right))
    }

    pub fn implies(left: Expr, right: Expr) -> Self {
        Expr::BinOp(BinOpKind::Implies, box left, box right, Position::default())
    }

    pub fn forall(vars: Vec<LocalVar>, triggers: Vec<Trigger>, body: Expr) -> Self {
        Expr::ForAll(vars, triggers, box body, Position::default())
    }

    pub fn ite(guard: Expr, left: Expr, right: Expr) -> Self {
        Expr::Cond(box guard, box left, box right, Position::default())
    }

    pub fn unfolding(
        pred_name: String,
        args: Vec<Expr>,
        expr: Expr,
        perm: PermAmount,
        variant: MaybeEnumVariantIndex,
    ) -> Self {
        Expr::Unfolding(pred_name, args, box expr, perm, variant, Position::default())
    }

    /// Create `unfolding T(arg) in body` where `T` is the type of `arg`.
    pub fn wrap_in_unfolding(arg: Expr, body: Expr) -> Expr {
        let type_name = arg.get_type().name();
        let pos = body.pos().clone();
        Expr::Unfolding(type_name, vec![arg], box body, PermAmount::Read, None, pos)
    }

    pub fn func_app(
        name: String,
        args: Vec<Expr>,
        internal_args: Vec<LocalVar>,
        return_type: Type,
        pos: Position,
    ) -> Self {
        Expr::FuncApp(name, args, internal_args, return_type, pos)
    }

    pub fn seq_index(seq: Expr, index: Expr) -> Self {
        Expr::check_seq_access(&seq);
        Expr::SeqIndex(box seq, box index, Position::default())
    }

    pub fn seq_len(seq: Expr) -> Self {
        Expr::check_seq_access(&seq);
        Expr::SeqLen(box seq, Position::default())
    }

    pub fn magic_wand(lhs: Expr, rhs: Expr, borrow: Option<Borrow>) -> Self {
        Expr::MagicWand(box lhs, box rhs, borrow, Position::default())
    }

    pub fn find(&self, sub_target: &Expr) -> bool {
        pub struct ExprFinder<'a> {
            sub_target: &'a Expr,
            found: bool,
        }
        impl<'a> ExprWalker for ExprFinder<'a> {
            fn walk(&mut self, expr: &Expr) {
                if expr == self.sub_target || (expr.is_place() && expr == self.sub_target) {
                    self.found = true;
                } else {
                    default_walk_expr(self, expr)
                }
            }
        }

        let mut finder = ExprFinder {
            sub_target,
            found: false,
        };
        finder.walk(self);
        finder.found
    }

    /// Extract all predicates places mentioned in the expression whose predicates have the given
    /// permission amount.
    pub fn extract_predicate_places(&self, perm_amount: PermAmount) -> Vec<Expr> {
        pub struct PredicateFinder {
            predicates: Vec<Expr>,
            perm_amount: PermAmount,
        }
        impl ExprWalker for PredicateFinder {
            fn walk_predicate_access_predicate(
                &mut self,
                _name: &str,
                arg: &Expr,
                perm_amount: PermAmount,
                _pos: &Position
            ) {
                if perm_amount == self.perm_amount {
                    self.predicates.push(arg.clone());
                }
            }
        }

        let mut finder = PredicateFinder {
            predicates: Vec::new(),
            perm_amount: perm_amount,
        };
        finder.walk(self);
        finder.predicates
    }

    /// Split place into place components.
    pub fn explode_place(&self) -> (Expr, Vec<PlaceComponent>) {
        match self {
            Expr::Variant(ref base, ref variant, ref pos) => {
                let (base_base, mut components) = base.explode_place();
                components.push(PlaceComponent::Variant(variant.clone(), pos.clone()));
                (base_base, components)
            }
            Expr::Field(ref base, ref field, ref pos) => {
                let (base_base, mut components) = base.explode_place();
                components.push(PlaceComponent::Field(field.clone(), pos.clone()));
                (base_base, components)
            }
            Expr::SeqIndex(ref base, ref index, ref pos) => {
                let (base_base, mut components) = base.explode_place();
                components.push(PlaceComponent::SeqIndex(index.clone(), pos.clone()));
                (base_base, components)
            }
            _ => (self.clone(), vec![]),
        }
    }

    /// Reconstruct place from the place components.
    pub fn reconstruct_place(self, components: Vec<PlaceComponent>) -> Expr {
        components
            .into_iter()
            .fold(self, |acc, component| match component {
                PlaceComponent::Variant(variant, pos) => Expr::Variant(box acc, variant, pos),
                PlaceComponent::Field(field, pos) => Expr::Field(box acc, field, pos),
                PlaceComponent::SeqIndex(index, pos) => Expr::SeqIndex(box acc, index, pos),
            })
    }

    // Methods from the old `Place` structure

    pub fn local(local: LocalVar) -> Self {
        Expr::Local(local, Position::default())
    }

    pub fn variant(self, index: &str) -> Self {
        assert!(self.is_place());
        let field_name = format!("enum_{}", index);
        let typ = self.get_type();
        let variant = Field::new(field_name, typ.clone().variant(index));
        Expr::Variant(box self, variant, Position::default())
    }

    pub fn field(self, field: Field) -> Self {
        Expr::Field(box self, field, Position::default())
    }

    pub fn addr_of(self) -> Self {
        let type_name = self.get_type().name();
        Expr::AddrOf(box self, Type::TypedRef(type_name), Position::default())
    }

    pub fn is_only_permissions(&self) -> bool {
        match self {
            Expr::PredicateAccessPredicate(..) |
            Expr::FieldAccessPredicate(..) |
            Expr::QuantifiedResourceAccess(..) => true,
            Expr::BinOp(BinOpKind::And, box lhs, box rhs, _) => {
                lhs.is_only_permissions() && rhs.is_only_permissions()
            }
            _ => false,
        }
    }

    pub fn is_place(&self) -> bool {
        match self {
            &Expr::Local(_, _) => true,
            &Expr::Variant(ref base, _, _)
            | &Expr::Field(ref base, _, _)
            | &Expr::AddrOf(ref base, _, _)
            | &Expr::LabelledOld(_, ref base, _)
            | &Expr::Unfolding(_, _, ref base, _, _, _)
            | &Expr::SeqIndex(ref base, _, _) => base.is_place(),
            _ => false,
        }
    }

    pub fn is_variant(&self) -> bool {
        match self {
            Expr::Variant(..) => true,
            _ => false,
        }
    }

    /// How many parts this place has? Used for ordering places.
    pub fn place_depth(&self) -> u32 {
        match self {
            &Expr::Local(_, _) => 1,
            &Expr::Variant(ref base, _, _)
            | &Expr::Field(ref base, _, _)
            | &Expr::AddrOf(ref base, _, _)
            | &Expr::LabelledOld(_, ref base, _)
            | &Expr::Unfolding(_, _, ref base, _, _, _)
            | &Expr::SeqIndex(ref base, _, _) => base.place_depth() + 1,
            x => unreachable!("{:?}", x),
        }
    }

    pub fn is_simple_place(&self) -> bool {
        match self {
            &Expr::Local(_, _) => true,
            &Expr::Variant(ref base, _, _)
            | &Expr::Field(ref base, _, _)
            | &Expr::SeqIndex(ref base, _, _) => base.is_simple_place(),
            _ => false,
        }
    }

    /// Only defined for places
    pub fn get_parent_ref(&self) -> Option<&Expr> {
        debug_assert!(self.is_place());
        match self {
            &Expr::Local(_, _) => None,
            &Expr::Variant(box ref base, _, _)
            | &Expr::Field(box Expr::SeqIndex(box ref base, _, _), _, _)
            | &Expr::Field(box ref base, _, _)
            | &Expr::AddrOf(box ref base, _, _) => Some(base),
            &Expr::LabelledOld(_, _, _) => None,
            &Expr::Unfolding(_, _, _, _, _, _) => None,
            ref x => unreachable!("{}", x),
        }
    }

    /// Only defined for places
    pub fn get_parent(&self) -> Option<Expr> {
        self.get_parent_ref().cloned()
    }

    /// Is this place a MIR reference?
    pub fn is_mir_reference(&self) -> bool {
        debug_assert!(self.is_place());
        if let Expr::Field(box Expr::Local(LocalVar { typ, .. }, _), _, _) = self {
            if let Type::TypedRef(ref name) = typ {
                // FIXME: We should not rely on string names for detecting types.
                return name.starts_with("ref$");
            }
        }
        false
    }

    /// If self is a MIR reference, dereference it.
    pub fn try_deref(&self) -> Option<Self> {
        if let Type::TypedRef(ref predicate_name) = self.get_type() {
            // FIXME: We should not rely on string names for type conversions.
            if predicate_name.starts_with("ref$") {
                let field_predicate_name = predicate_name[4..predicate_name.len()].to_string();
                let field = Field::new("val_ref", Type::TypedRef(field_predicate_name));
                let field_place = Expr::from(self.clone()).field(field);
                return Some(field_place);
            }
        }
        None
    }

    pub fn is_local(&self) -> bool {
        match self {
            &Expr::Local(..) => true,
            _ => false,
        }
    }

    pub fn is_addr_of(&self) -> bool {
        match self {
            &Expr::AddrOf(..) => true,
            _ => false,
        }
    }

    /// Puts an `old[label](..)` around the expression
    pub fn old<S: fmt::Display + ToString>(self, label: S) -> Self {
        match self {
            Expr::Local(..) => {
                /*
                debug!(
                    "Trying to put an old expression 'old[{}](..)' around {}, which is a local variable",
                    label,
                    self
                );
                */
                self
            }
            Expr::LabelledOld(..) => {
                /*
                debug!(
                    "Trying to put an old expression 'old[{}](..)' around {}, which already has a label",
                    label,
                    self
                );
                */
                self
            }
            _ => Expr::LabelledOld(label.to_string(), box self, Position::default()),
        }
    }

    pub fn is_old(&self) -> bool {
        self.get_label().is_some()
    }

    pub fn is_curr(&self) -> bool {
        !self.is_old()
    }

    pub fn get_place(&self) -> Option<&Expr> {
        match self {
            Expr::PredicateAccessPredicate(_, ref arg, _, _) => Some(arg),
            Expr::FieldAccessPredicate(box ref arg, _, _) => Some(arg),
            Expr::QuantifiedResourceAccess(quant, _) => Some(quant.resource.get_place()),
            _ => None,
        }
    }

    pub fn get_perm_amount(&self) -> PermAmount {
        match self {
            Expr::PredicateAccessPredicate(_, _, perm_amount, _) => *perm_amount,
            Expr::FieldAccessPredicate(_, perm_amount, _) => *perm_amount,
            Expr::QuantifiedResourceAccess(quant, _) => quant.resource.get_perm_amount(),
            x => unreachable!("{}", x),
        }
    }

    pub fn is_pure(&self) -> bool {
        struct PurityFinder {
            non_pure: bool,
        }
        impl ExprWalker for PurityFinder {
            fn walk_predicate_access_predicate(
                &mut self,
                _name: &str,
                _arg: &Expr,
                _perm_amount: PermAmount,
                _pos: &Position
            ) {
                self.non_pure = true;
            }
            fn walk_field_access_predicate(
                &mut self,
                _receiver: &Expr,
                _perm_amount: PermAmount,
                _pos: &Position
            ) {
                self.non_pure = true;
            }
        }
        let mut walker = PurityFinder { non_pure: false };
        walker.walk(self);
        !walker.non_pure
    }

    /// Only defined for places
    pub fn get_base(&self) -> LocalVar {
        debug_assert!(self.is_place());
        match self {
            &Expr::Local(ref var, _) => var.clone(),
            &Expr::LabelledOld(_, ref base, _) |
            &Expr::Unfolding(_, _, ref base, _, _, _) => {
                base.get_base()
            }
            _ => self.get_parent().unwrap().get_base(),
        }
    }

    pub fn get_label(&self) -> Option<&String> {
        match self {
            &Expr::LabelledOld(ref label, _, _) => Some(label),
            _ => None,
        }
    }

    /* Moved to the Eq impl
    /// Place equality after type elision
    pub fn weak_eq(&self, other: &Expr) -> bool {
        debug_assert!(self.is_place());
        debug_assert!(other.is_place());
        match (self, other) {
            (
                Expr::Local(ref self_var),
                Expr::Local(ref other_var)
            ) => self_var.weak_eq(other_var),
            (
                Expr::Field(box ref self_base, ref self_field),
                Expr::Field(box ref other_base, ref other_field)
            ) => self_field.weak_eq(other_field) && self_base.weak_eq(other_base),
            (
                Expr::AddrOf(box ref self_base, ref self_typ),
                Expr::AddrOf(box ref other_base, ref other_typ)
            ) => self_typ.weak_eq(other_typ) && self_base.weak_eq(other_base),
            (
                Expr::LabelledOld(ref self_label, box ref self_base),
                Expr::LabelledOld(ref other_label, box ref other_base)
            ) => self_label == other_label && self_base.weak_eq(other_base),
            (
                Expr::Unfolding(ref self_name, ref self_args, box ref self_base, self_frac),
                Expr::Unfolding(ref other_name, ref other_args, box ref other_base, other_frac)
            ) => self_name == other_name && self_frac == other_frac &&
                self_args[0].weak_eq(&other_args[0]) && self_base.weak_eq(other_base),
            _ => false
        }
    }
    */

    pub fn has_proper_prefix(&self, other: &Expr) -> bool {
        debug_assert!(self.is_place(), "self={} other={}", self, other);
        debug_assert!(other.is_place(), "self={} other={}", self, other);
        self != other && self.has_prefix(other)
    }

    pub fn has_prefix(&self, other: &Expr) -> bool {
        debug_assert!(self.is_place());
        debug_assert!(other.is_place());
        if self == other {
            true
        } else {
            match self.get_parent() {
                Some(parent) => parent.has_prefix(other),
                None => false,
            }
        }
    }

    pub fn all_proper_prefixes(&self) -> Vec<Expr> {
        debug_assert!(self.is_place());
        match self.get_parent() {
            Some(parent) => parent.all_prefixes(),
            None => vec![],
        }
    }

    // Returns all prefixes, from the shortest to the longest
    pub fn all_prefixes(&self) -> Vec<Expr> {
        debug_assert!(self.is_place());
        let mut res = self.all_proper_prefixes();
        res.push(self.clone());
        res
    }

    pub fn get_type(&self) -> Type {
        debug_assert!(self.is_place());
        match self {
            &Expr::Local(LocalVar { ref typ, .. }, _)
            | &Expr::Variant(_, Field { ref typ, .. }, _)
            | &Expr::Field(_, Field { ref typ, .. }, _)
            | &Expr::AddrOf(_, ref typ, _) => {
                typ.clone()
            },
            &Expr::LabelledOld(_, box ref base, _)
            | &Expr::Unfolding(_, _, box ref base, _, _, _) => {
                base.get_type()
            }
            &Expr::SeqIndex(box ref base, _, _)=> {
                return match base.get_type() {
                    Type::TypedSeq(struct_pred) => Type::TypedRef(struct_pred),
                    x => unreachable!("Got {:?}", x),
                }
            }
            _ => panic!(),
        }
    }

    /// If returns true, then the expression is guaranteed to be boolean. However, it may return
    /// false even for boolean expressions.
    pub fn is_bool(&self) -> bool {
        if self.is_place() {
            self.get_type() == Type::Bool
        } else {
            match self {
                Expr::Const(Const::Bool(_), _) |
                Expr::UnaryOp(UnaryOpKind::Not, _, _) |
                Expr::FuncApp(_, _, _, Type::Bool, _) |
                Expr::ForAll(..) => {
                    true
                },
                Expr::BinOp(kind, _, _, _) => {
                    use self::BinOpKind::*;
                    *kind == EqCmp || *kind == NeCmp ||
                    *kind == GtCmp || *kind == GeCmp || *kind == LtCmp || *kind == LeCmp ||
                    *kind == And || *kind == Or || *kind == Implies
                },
                _ => {
                    false
                }
            }
        }
    }

    pub fn typed_ref_name(&self) -> Option<String> {
        match self.get_type() {
            Type::TypedRef(name) => Some(name),
            _ => None,
        }
    }

    pub fn map_labels<F>(self, f: F) -> Self
    where
        F: Fn(String) -> Option<String>,
    {
        struct OldLabelReplacer<T: Fn(String) -> Option<String>> {
            f: T,
        };
        impl<T: Fn(String) -> Option<String>> ExprFolder for OldLabelReplacer<T> {
            fn fold_labelled_old(&mut self, label: String, base: Box<Expr>, pos: Position) -> Expr {
                match (self.f)(label) {
                    Some(new_label) => base.old(new_label).set_pos(pos),
                    None => *base,
                }
            }
        }
        OldLabelReplacer { f }.fold(self)
    }

    pub fn replace_place(self, target: &Expr, replacement: &Expr) -> Self {
        debug_assert!(target.is_place());
        //assert_eq!(target.get_type(), replacement.get_type());
        if replacement.is_place() {
            assert!(
                target.get_type() == replacement.get_type(),
                "Cannot substitute '{}' with '{}', because they have incompatible types '{}' and '{}'",
                target,
                replacement,
                target.get_type(),
                replacement.get_type()
            );
        }
        struct PlaceReplacer<'a> {
            target: &'a Expr,
            replacement: &'a Expr,
            // FIXME: the following fields serve a grotesque hack.
            //  Purpose:  Generics. When a less-generic function-under-test desugars specs from
            //            a more-generic function, the vir::Expr contains Local's with __TYPARAM__s,
            //            but Field's with the function-under-test's concrete types. The purpose is
            //            the to "fix" the (Viper) predicates of the fields, i.e. replace those
            //            typarams with local (more) concrete types.
            //            THIS IS FRAGILE!
            typaram_substs: Option<typaram::Substs>,
            subst: bool,
        };
        impl<'a> ExprFolder for PlaceReplacer<'a> {
            fn fold(&mut self, e: Expr) -> Expr {
                if e.is_place() && &e == self.target {
                    self.subst = true;
                    self.replacement.clone()
                } else {
                    match default_fold_expr(self, e) {
                        Expr::Field(expr, mut field, pos) => {
                            if let Some(ts) = &self.typaram_substs {
                                if self.subst && field.typ.is_ref() {
                                    let inner1 = field.typ.name();
                                    let inner2 = ts.apply(&inner1);
                                    debug!("replacing:\n{}\n{}\n========", &inner1, &inner2);
                                    field = Field::new(field.name, Type::TypedRef(inner2));
                                }
                            }
                            Expr::Field(expr, field, pos)
                        }
                        x => {
                            self.subst = false;
                            x
                        }
                    }
                }
            }

            fn fold_forall(
                &mut self,
                vars: Vec<LocalVar>,
                triggers: Vec<Trigger>,
                body: Box<Expr>,
                pos: Position,
            ) -> Expr {
                if vars.contains(&self.target.get_base()) {
                    // Do nothing
                    Expr::ForAll(vars, triggers, body, pos)
                } else {
                    Expr::ForAll(
                        vars,
                        triggers
                            .into_iter()
                            .map(|x| x.replace_place(self.target, self.replacement))
                            .collect(),
                        self.fold_boxed(body),
                        pos,
                    )
                }
            }
        }
        let typaram_substs = match (&target, &replacement) {
            (Expr::Local(tv, _), Expr::Local(rv, _)) => {
                if tv.typ.is_ref() && rv.typ.is_ref() {
                    debug!(
                        "learning:\n{}\n{}\n=======",
                        &target.local_type(),
                        replacement.local_type()
                    );
                    Some(typaram::Substs::learn(
                        &target.local_type(),
                        &replacement.local_type(),
                    ))
                } else {
                    None
                }
            }
            _ => None,
        };
        PlaceReplacer {
            target,
            replacement,
            typaram_substs,
            subst: false,
        }
        .fold(self)
    }

    /// Replaces expressions like `old[l5](old[l5](_9.val_ref).foo.bar)`
    /// into `old[l5](_9.val_ref.foo.bar)`
    pub fn remove_redundant_old(self) -> Self {
        struct RedundantOldRemover {
            current_label: Option<String>,
        };
        impl ExprFolder for RedundantOldRemover {
            fn fold_labelled_old(&mut self, label: String, base: Box<Expr>, pos: Position) -> Expr {
                let old_current_label = mem::replace(&mut self.current_label, Some(label.clone()));
                let new_base = default_fold_expr(self, *base);
                let new_expr = if Some(label.clone()) == old_current_label {
                    new_base
                } else {
                    new_base.old(label).set_pos(pos)
                };
                self.current_label = old_current_label;
                new_expr
            }
        }
        RedundantOldRemover {
            current_label: None,
        }
        .fold(self)
    }

    /// Leaves a conjunction of `acc(..)` expressions
    pub fn filter_perm_conjunction(self) -> Self {
        struct PermConjunctionFilter();
        impl ExprFolder for PermConjunctionFilter {
            fn fold(&mut self, e: Expr) -> Expr {
                match e {
                    f @ Expr::PredicateAccessPredicate(..) => f,
                    f @ Expr::FieldAccessPredicate(..) => f,
                    f @ Expr::QuantifiedResourceAccess(..) => f,
                    Expr::BinOp(BinOpKind::And, y, z, p) => {
                        self.fold_bin_op(BinOpKind::And, y, z, p)
                    }

                    Expr::BinOp(..)
                    | Expr::MagicWand(..)
                    | Expr::Unfolding(..)
                    | Expr::Cond(..)
                    | Expr::UnaryOp(..)
                    | Expr::Const(..)
                    | Expr::Local(..)
                    | Expr::Variant(..)
                    | Expr::Field(..)
                    | Expr::AddrOf(..)
                    | Expr::LabelledOld(..)
                    | Expr::ForAll(..)
                    | Expr::LetExpr(..)
                    | Expr::FuncApp(..)
                    | Expr::SeqIndex(..)
                    | Expr::SeqLen(..) => true.into(),
                }
            }
        }
        PermConjunctionFilter().fold(self)
    }

    /// Apply the closure to all places in the expression.
    pub fn fold_places<F>(self, f: F) -> Expr
    where
        F: Fn(Expr) -> Expr,
    {
        struct PlaceFolder<F>
        where
            F: Fn(Expr) -> Expr,
        {
            f: F,
        };
        impl<F> ExprFolder for PlaceFolder<F>
        where
            F: Fn(Expr) -> Expr,
        {
            fn fold(&mut self, e: Expr) -> Expr {
                if e.is_place() {
                    (self.f)(e)
                } else {
                    default_fold_expr(self, e)
                }
            }
            // TODO: Handle triggers?
        }
        PlaceFolder { f }.fold(self)
    }

    /// Apply the closure to all expressions.
    pub fn fold_expr<F>(self, f: F) -> Expr
    where
        F: Fn(Expr) -> Expr,
    {
        struct ExprFolderImpl<F>
        where
            F: Fn(Expr) -> Expr,
        {
            f: F,
        };
        impl<F> ExprFolder for ExprFolderImpl<F>
        where
            F: Fn(Expr) -> Expr,
        {
            fn fold(&mut self, e: Expr) -> Expr {
                let new_expr = default_fold_expr(self, e);
                (self.f)(new_expr)
            }
        }
        ExprFolderImpl { f }.fold(self)
    }

    pub fn local_type(&self) -> String {
        match &self {
            Expr::Local(localvar, _) => match &localvar.typ {
                Type::TypedRef(str) => str.clone(),
                _ => panic!("expected Type::TypedRef"),
            },
            _ => panic!("expected Expr::Local"),
        }
    }

    /// Compute the permissions that are needed for this expression to
    /// be successfully evaluated. This is method is used for `fold` and
    /// `exhale` statements inside `package` statements because Silicon
    /// fails to compute which permissions it should take into the magic
    /// wand.
    pub fn compute_footprint(&self, perm_amount: PermAmount) -> Vec<Expr> {
        struct Collector {
            perm_amount: PermAmount,
            perms: Vec<Expr>,
        }
        impl ExprWalker for Collector {
            fn walk_variant(&mut self, e: &Expr, v: &Field, p: &Position) {
                self.walk(e);
                let expr = Expr::Variant(box e.clone(), v.clone(), p.clone());
                let perm = Expr::acc_permission(expr, self.perm_amount);
                self.perms.push(perm);
            }
            fn walk_field(&mut self, e: &Expr, f: &Field, p: &Position) {
                self.walk(e);
                let expr = Expr::Field(box e.clone(), f.clone(), p.clone());
                let perm = Expr::acc_permission(expr, self.perm_amount);
                self.perms.push(perm);
            }
            fn walk_labelled_old(&mut self, _label: &str, _expr: &Expr, _pos: &Position) {
                // Stop recursion.
            }
        }
        let mut collector = Collector {
            perm_amount: perm_amount,
            perms: Vec::new(),
        };
        collector.walk(self);
        collector.perms
    }

    /// FIXME: A hack. Replaces all generic types with their instantiations by using string
    /// substitution.
    pub fn patch_types(self, substs: &HashMap<String, String>) -> Self {
        struct TypePatcher<'a> {
            substs: &'a HashMap<String, String>,
        }
        impl<'a> ExprFolder for TypePatcher<'a> {
            fn fold_predicate_access_predicate(
                &mut self,
                mut predicate_name: String,
                arg: Box<Expr>,
                perm_amount: PermAmount,
                pos: Position,
            ) -> Expr {
                for (typ, subst) in self.substs {
                    predicate_name = predicate_name.replace(typ, subst);
                }
                Expr::PredicateAccessPredicate(
                    predicate_name,
                    self.fold_boxed(arg),
                    perm_amount,
                    pos,
                )
            }
            fn fold_local(&mut self, mut var: LocalVar, pos: Position) -> Expr {
                var.typ = var.typ.patch(self.substs);
                Expr::Local(var, pos)
            }
            fn fold_func_app(
                &mut self,
                name: String,
                args: Vec<Expr>,
                formal_args: Vec<LocalVar>,
                return_type: Type,
                pos: Position,
            ) -> Expr {
                let formal_args = formal_args
                    .into_iter()
                    .map(|mut var| {
                        var.typ = var.typ.patch(self.substs);
                        var
                    })
                    .collect();
                // FIXME: We do not patch the return_type because pure functions cannot return
                // generic values.
                Expr::FuncApp(
                    name,
                    args.into_iter().map(|e| self.fold(e)).collect(),
                    formal_args,
                    return_type,
                    pos,
                )
            }
        }
        let mut patcher = TypePatcher { substs: substs };
        patcher.fold(self)
    }

    // TODO: misleading name
    pub fn rename_single(self, old: &LocalVar, new: LocalVar) -> Self {
        self.rename(&Some((old.clone(), new)).into_iter().collect())
    }

    pub fn rename(self, mapping: &HashMap<LocalVar, LocalVar>) -> Self {
        if mapping.is_empty() {
            self
        } else {
            self.subst_vars(
                &mapping.iter()
                    .map(|(lhs_lv, rhs_lv)|
                        (lhs_lv.clone(), Expr::Local(rhs_lv.clone(), Position::default()))
                    ).collect()
            )
        }
    }

    // TODO: Test this
    pub fn subst_vars(self, subst_map: &HashMap<LocalVar, Expr>) -> Self {
        struct SubstVar<'a> {
            subst_map: &'a HashMap<LocalVar, Expr>,
            excluding: HashSet<LocalVar>
        }
        impl<'a> ExprFolder for SubstVar<'a> {
            fn fold_local(&mut self, v: LocalVar, p: Position) -> Expr {
                if !self.excluding.contains(&v) {
                    self.subst_map.get(&v).cloned().unwrap_or(Expr::Local(v, p))
                } else {
                    Expr::Local(v, p)
                }
            }

            fn fold_forall(&mut self, vars: Vec<LocalVar>, triggers: Vec<Trigger>, body: Box<Expr>, p: Position) -> Expr {
                vars.iter().for_each(|v| { self.excluding.insert(v.clone()); });
                let folded_body = self.fold_boxed(body);
                vars.iter().for_each(|v| { self.excluding.remove(v); });
                Expr::ForAll(vars, triggers, self.fold_boxed(folded_body), p)
            }

            fn fold_let_expr(&mut self, var: LocalVar, expr: Box<Expr>, body: Box<Expr>, pos: Position) -> Expr {
                self.excluding.insert(var.clone());
                let folded_expr = self.fold_boxed(expr);
                let folded_body = self.fold_boxed(body);
                self.excluding.remove(&var);
                Expr::LetExpr(var, folded_expr, folded_body, pos)
            }

            fn fold_quantified_resource_access(&mut self, quant: QuantifiedResourceAccess, p: Position) -> Expr {
                quant.vars.iter().for_each(|v| { self.excluding.insert(v.clone()); });
                let folded_cond = self.fold_boxed(quant.cond);
                let folded_resource = quant.resource.map_expression(|e| self.fold(e));
                quant.vars.iter().for_each(|v| { self.excluding.remove(v); });
                Expr::QuantifiedResourceAccess(QuantifiedResourceAccess {
                    vars: quant.vars,
                    triggers: quant.triggers,
                    cond: folded_cond,
                    resource: folded_resource
                }, p)
            }
        }

        if subst_map.is_empty() {
            self
        } else {
            SubstVar {
                subst_map,
                excluding: HashSet::new()
            }.fold(self)
        }
    }

    pub fn subst(self, subst_map: &HashMap<Expr, Expr>) -> Self {
        if subst_map.is_empty() {
            self
        } else {
            self.fold_expr(|e| subst_map.get(&e).unwrap_or(&e).clone())
        }
    }

    pub fn depth(&self) -> usize {
        use std::cmp::max;
        match self {
            Expr::Local(_, _) => 1,
            Expr::Variant(base, _, _) => 1 + base.depth(),
            Expr::Field(base, _, _) => 1 + base.depth(),
            Expr::AddrOf(base, _, _) => 1 + base.depth(),
            Expr::LabelledOld(_, base, _) => 1 + base.depth(),
            Expr::Const(_, _) => 1,
            Expr::MagicWand(lhs, rhs, _, _) => 1 + max(lhs.depth(), rhs.depth()),
            Expr::PredicateAccessPredicate(_, arg, _, _) => 1 + arg.depth(),
            Expr::FieldAccessPredicate(arg, _, _) => 1 + arg.depth(),
            Expr::UnaryOp(_, arg, _) => 1 + arg.depth(),
            Expr::BinOp(_, lhs, rhs, _) => 1 + max(lhs.depth(), rhs.depth()),
            Expr::Unfolding(_, predicate_args, in_expr, _, _, _) =>
                1 + max(
                    in_expr.depth(),
                    predicate_args.iter().map(|e| e.depth()).max().unwrap_or(0)
                ),
            Expr::Cond(guard, then_expr, else_expr, _) =>
                1 + max(guard.depth(), max(then_expr.depth(), else_expr.depth())),
            Expr::ForAll(_, _, body, _) => 1 + body.depth(),
            Expr::LetExpr(_, defexpr, body, _) =>
                1 + max(defexpr.depth(), body.depth()),
            Expr::FuncApp(_, args, _, _, _) =>
                1 + args.iter().map(|e| e.depth()).max().unwrap_or(0),
            Expr::SeqIndex(seq, index, _) =>  1 + max(seq.depth(), index.depth()),
            Expr::SeqLen(seq, _) => 1 + seq.depth(),
            Expr::QuantifiedResourceAccess(quant, _) =>
                1 + max(quant.cond.depth(), quant.resource.get_place().depth()),
        }
    }

    pub fn get_local_vars(&self, lvs: &HashSet<LocalVar>) -> HashSet<LocalVar> {
        fn inner<'a>(
            expr: &Expr,
            lvs: &HashSet<LocalVar>,
            exclude: &mut HashSet<LocalVar>,
            result: &mut HashSet<LocalVar>,
        ) {
            match expr {
                Expr::Local(lv, _) => {
                    if lvs.contains(lv) && !exclude.contains(lv) {
                        result.insert(lv.clone());
                    }
                }
                Expr::Variant(base, _, _) => inner(base, lvs, exclude, result),
                Expr::Field(base, _, _) => inner(base, lvs, exclude, result),
                Expr::AddrOf(base, _, _) => inner(base, lvs, exclude, result),
                Expr::LabelledOld(_, base, _) => inner(base, lvs, exclude, result),
                Expr::UnaryOp(_, base, _) => inner(base, lvs, exclude, result),
                Expr::PredicateAccessPredicate(_, base, _, _) => inner(base, lvs, exclude, result),
                Expr::FieldAccessPredicate(base, _, _) => inner(base, lvs, exclude, result),
                Expr::SeqLen(base, _) => inner(base, lvs, exclude, result),
                Expr::MagicWand(lhs, rhs, _, _) => {
                    inner(lhs, lvs, exclude, result);
                    inner(rhs, lvs, exclude, result);
                }
                Expr::BinOp(_, lhs, rhs, _) => {
                    inner(lhs, lvs, exclude, result);
                    inner(rhs, lvs, exclude, result);
                },
                Expr::Unfolding(_, args, expr, _, _, _) => {
                    args.iter().for_each(|e| inner(e, lvs, exclude, result));
                    inner(expr, lvs, exclude, result);
                }
                Expr::Cond(cond, then, els, _) => {
                    inner(cond, lvs, exclude, result);
                    inner(then, lvs, exclude, result);
                    inner(els, lvs, exclude, result);
                }
                Expr::ForAll(vars, _, base, _) => {
                    vars.iter().for_each(|lv| { exclude.insert(lv.clone()); });
                    inner(base, lvs, exclude, result);
                    vars.iter().for_each(|lv| { exclude.remove(lv); });
                },
                Expr::LetExpr(var, def, expr, _) => {
                    exclude.insert(var.clone());
                    inner(def, lvs, exclude, result);
                    inner(expr, lvs, exclude, result);
                    exclude.remove(var);
                },
                Expr::FuncApp(_, args, formal_args, _, _) => {
                    formal_args.iter().for_each(|lv| { exclude.insert(lv.clone()); });
                    args.iter().for_each(|e| inner(e, lvs, exclude, result));
                    formal_args.iter().for_each(|lv| { exclude.remove(lv); });
                },
                Expr::SeqIndex(seq, index, _) => {
                    inner(seq, lvs, exclude, result);
                    inner(index, lvs, exclude, result);
                }
                Expr::QuantifiedResourceAccess(quant, _) =>
                    inner(&quant.to_forall_expression(), lvs, exclude, result),
                Expr::Const(_, _) => (),
            }
        }
        let mut result = HashSet::new();
        if lvs.is_empty() {
            // Just to avoid unnecessary traversing
            return result;
        }
        inner(self, lvs, &mut HashSet::new(), &mut result);
        result
    }

    pub fn contains_any_var(&self, lvs: &HashSet<LocalVar>) -> bool {
        !lvs.is_empty() && !self.get_local_vars(lvs).is_empty()
    }

    /// Extract the sequence and the indexing of the expression
    /// Example: for `x.a.b.val_array[idx].val_ref`, it will return `Some((x.a.b.val_array, idx))`
    pub fn extract_seq_and_index(&self) -> Option<(&Expr, &Expr)> {
        match self {
            Expr::Field(box Expr::SeqIndex(box ref seq, box ref index, _), _, _) =>
                Some((seq, index)),
            // See comment of Expr::SeqIndex
            Expr::SeqIndex(..) =>
                panic!("SeqIndex is ill-formed"),
            _ => None
        }
    }

    fn check_seq_access(seq: &Expr) {
        match seq {
            Expr::Field(_, Field { name, typ }, _)
                if name.as_str() == "val_array" && typ.get_id() == TypeId::Seq => (),
            x => panic!("`seq` must be a field access of val_array (got {})", x)
        }
    }
}

impl PartialEq for Expr {
    /// Compare ignoring the `position` field
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Expr::Local(ref self_var, _), Expr::Local(ref other_var, _)) => self_var == other_var,
            (
                Expr::Variant(box ref self_base, ref self_variant, _),
                Expr::Variant(box ref other_base, ref other_variant, _),
            ) => (self_base, self_variant) == (other_base, other_variant),
            (
                Expr::Field(box ref self_base, ref self_field, _),
                Expr::Field(box ref other_base, ref other_field, _),
            ) => (self_base, self_field) == (other_base, other_field),
            (
                Expr::AddrOf(box ref self_base, ref self_typ, _),
                Expr::AddrOf(box ref other_base, ref other_typ, _),
            ) => (self_base, self_typ) == (other_base, other_typ),
            (
                Expr::LabelledOld(ref self_label, box ref self_base, _),
                Expr::LabelledOld(ref other_label, box ref other_base, _),
            ) => (self_label, self_base) == (other_label, other_base),
            (Expr::Const(ref self_const, _), Expr::Const(ref other_const, _)) => {
                self_const == other_const
            }
            (
                Expr::MagicWand(box ref self_lhs, box ref self_rhs, self_borrow, _),
                Expr::MagicWand(box ref other_lhs, box ref other_rhs, other_borrow, _),
            ) => (self_lhs, self_rhs, self_borrow) == (other_lhs, other_rhs, other_borrow),
            (
                Expr::PredicateAccessPredicate(ref self_name, ref self_arg, self_perm, _),
                Expr::PredicateAccessPredicate(ref other_name, ref other_arg, other_perm, _),
            ) => (self_name, self_arg, self_perm) == (other_name, other_arg, other_perm),
            (
                Expr::FieldAccessPredicate(box ref self_base, self_perm, _),
                Expr::FieldAccessPredicate(box ref other_base, other_perm, _),
            ) => (self_base, self_perm) == (other_base, other_perm),
            (
                Expr::UnaryOp(self_op, box ref self_arg, _),
                Expr::UnaryOp(other_op, box ref other_arg, _),
            ) => (self_op, self_arg) == (other_op, other_arg),
            (
                Expr::BinOp(self_op, box ref self_left, box ref self_right, _),
                Expr::BinOp(other_op, box ref other_left, box ref other_right, _),
            ) => (self_op, self_left, self_right) == (other_op, other_left, other_right),
            (
                Expr::Cond(box ref self_cond, box ref self_then, box ref self_else, _),
                Expr::Cond(box ref other_cond, box ref other_then, box ref other_else, _),
            ) => (self_cond, self_then, self_else) == (other_cond, other_then, other_else),
            (
                Expr::ForAll(ref self_vars, ref self_triggers, box ref self_expr, _),
                Expr::ForAll(ref other_vars, ref other_triggers, box ref other_expr, _),
            ) => (self_vars, self_triggers, self_expr) == (other_vars, other_triggers, other_expr),
            (
                Expr::LetExpr(ref self_var, box ref self_def, box ref self_expr, _),
                Expr::LetExpr(ref other_var, box ref other_def, box ref other_expr, _),
            ) => (self_var, self_def, self_expr) == (other_var, other_def, other_expr),
            (
                Expr::FuncApp(ref self_name, ref self_args, _, _, _),
                Expr::FuncApp(ref other_name, ref other_args, _, _, _),
            ) => (self_name, self_args) == (other_name, other_args),
            (
                Expr::Unfolding(ref self_name, ref self_args, box ref self_base, self_perm, ref self_variant, _),
                Expr::Unfolding(ref other_name, ref other_args, box ref other_base, other_perm, ref other_variant, _),
            ) => {
                (self_name, self_args, self_base, self_perm, self_variant)
                    == (other_name, other_args, other_base, other_perm, other_variant)
            }
            (
                Expr::SeqIndex(ref self_seq, ref self_index, _),
                Expr::SeqIndex(ref other_seq, ref other_index, _),
            ) => (self_seq, self_index) == (other_seq, other_index),
            (
                Expr::SeqLen(ref self_seq, _),
                Expr::SeqLen(ref other_seq, _),
            ) => self_seq == other_seq,
            (
                Expr::QuantifiedResourceAccess(self_quant, _),
                Expr::QuantifiedResourceAccess(other_quant, _),
            ) => self_quant == other_quant,
            (a, b) => {
                debug_assert_ne!(discriminant(a), discriminant(b));
                false
            }
        }
    }
}

impl Eq for Expr {}

impl Hash for Expr {
    /// Hash ignoring the `position` field
    fn hash<H: Hasher>(&self, state: &mut H) {
        discriminant(self).hash(state);
        match self {
            Expr::Local(ref var, _) => var.hash(state),
            Expr::Variant(box ref base, variant_index, _) => (base, variant_index).hash(state),
            Expr::Field(box ref base, ref field, _) => (base, field).hash(state),
            Expr::AddrOf(box ref base, ref typ, _) => (base, typ).hash(state),
            Expr::LabelledOld(ref label, box ref base, _) => (label, base).hash(state),
            Expr::Const(ref const_expr, _) => const_expr.hash(state),
            Expr::MagicWand(box ref lhs, box ref rhs, b, _) => (lhs, rhs, b).hash(state),
            Expr::PredicateAccessPredicate(ref name, ref arg, perm, _) => {
                (name, arg, perm).hash(state)
            }
            Expr::FieldAccessPredicate(box ref base, perm, _) => (base, perm).hash(state),
            Expr::UnaryOp(op, box ref arg, _) => (op, arg).hash(state),
            Expr::BinOp(op, box ref left, box ref right, _) => (op, left, right).hash(state),
            Expr::Cond(box ref cond, box ref then_expr, box ref else_expr, _) => {
                (cond, then_expr, else_expr).hash(state)
            }
            Expr::ForAll(ref vars, ref triggers, box ref expr, _) => {
                (vars, triggers, expr).hash(state)
            }
            Expr::LetExpr(ref var, box ref def, box ref expr, _) => (var, def, expr).hash(state),
            Expr::FuncApp(ref name, ref args, _, _, _) => (name, args).hash(state),
            Expr::Unfolding(ref name, ref args, box ref base, perm, ref variant, _) => {
                (name, args, base, perm, variant).hash(state)
            }
            Expr::SeqIndex(ref seq, ref index, _) => (seq, index).hash(state),
            Expr::SeqLen(ref seq, _) => seq.hash(state),
            Expr::QuantifiedResourceAccess(ref quant, _) => quant.hash(state),
        }
    }
}

pub trait ExprFolder: Sized {
    fn fold(&mut self, e: Expr) -> Expr {
        default_fold_expr(self, e)
    }

    fn fold_boxed(&mut self, e: Box<Expr>) -> Box<Expr> {
        box self.fold(*e)
    }

    fn fold_local(&mut self, v: LocalVar, p: Position) -> Expr {
        Expr::Local(v, p)
    }
    fn fold_variant(&mut self, base: Box<Expr>, variant: Field, p: Position) -> Expr {
        Expr::Variant(self.fold_boxed(base), variant, p)
    }
    fn fold_field(&mut self, receiver: Box<Expr>, field: Field, pos: Position) -> Expr {
        Expr::Field(self.fold_boxed(receiver), field, pos)
    }
    fn fold_addr_of(&mut self, e: Box<Expr>, t: Type, p: Position) -> Expr {
        Expr::AddrOf(self.fold_boxed(e), t, p)
    }
    fn fold_const(&mut self, x: Const, p: Position) -> Expr {
        Expr::Const(x, p)
    }
    fn fold_labelled_old(
        &mut self,
        label: String,
        body: Box<Expr>,
        pos: Position
    ) -> Expr {
        Expr::LabelledOld(label, self.fold_boxed(body), pos)
    }
    fn fold_magic_wand(
        &mut self,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        borrow: Option<Borrow>,
        pos: Position,
    ) -> Expr {
        Expr::MagicWand(self.fold_boxed(lhs), self.fold_boxed(rhs), borrow, pos)
    }
    fn fold_predicate_access_predicate(
        &mut self,
        name: String,
        arg: Box<Expr>,
        perm_amount: PermAmount,
        pos: Position,
    ) -> Expr {
        Expr::PredicateAccessPredicate(name, self.fold_boxed(arg), perm_amount, pos)
    }
    fn fold_field_access_predicate(
        &mut self,
        receiver: Box<Expr>,
        perm_amount: PermAmount,
        pos: Position
    ) -> Expr {
        Expr::FieldAccessPredicate(self.fold_boxed(receiver), perm_amount, pos)
    }
    fn fold_unary_op(&mut self, x: UnaryOpKind, y: Box<Expr>, p: Position) -> Expr {
        Expr::UnaryOp(x, self.fold_boxed(y), p)
    }
    fn fold_bin_op(
        &mut self,
        kind: BinOpKind,
        first: Box<Expr>,
        second: Box<Expr>,
        pos: Position
    ) -> Expr {
        Expr::BinOp(kind, self.fold_boxed(first), self.fold_boxed(second), pos)
    }
    fn fold_unfolding(
        &mut self,
        name: String,
        args: Vec<Expr>,
        expr: Box<Expr>,
        perm: PermAmount,
        variant: MaybeEnumVariantIndex,
        pos: Position,
    ) -> Expr {
        Expr::Unfolding(
            name,
            args.into_iter().map(|e| self.fold(e)).collect(),
            self.fold_boxed(expr),
            perm,
            variant,
            pos,
        )
    }
    fn fold_cond(
        &mut self,
        guard: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        pos: Position
    ) -> Expr {
        Expr::Cond(
            self.fold_boxed(guard),
            self.fold_boxed(then_expr),
            self.fold_boxed(else_expr),
            pos,
        )
    }
    fn fold_forall(
        &mut self,
        x: Vec<LocalVar>,
        y: Vec<Trigger>,
        z: Box<Expr>,
        p: Position,
    ) -> Expr {
        Expr::ForAll(x, y, self.fold_boxed(z), p)
    }
    fn fold_let_expr(
        &mut self,
        var: LocalVar,
        expr: Box<Expr>,
        body: Box<Expr>,
        pos: Position
    ) -> Expr {
        Expr::LetExpr(var, self.fold_boxed(expr), self.fold_boxed(body), pos)
    }
    fn fold_func_app(
        &mut self,
        name: String,
        args: Vec<Expr>,
        formal_args: Vec<LocalVar>,
        return_type: Type,
        pos: Position,
    ) -> Expr {
        Expr::FuncApp(
            name,
            args.into_iter().map(|e| self.fold(e)).collect(),
            formal_args,
            return_type,
            pos
        )
    }
    fn fold_seq_index(&mut self, seq: Box<Expr>, index: Box<Expr>, p: Position) -> Expr {
        Expr::SeqIndex(self.fold_boxed(seq), self.fold_boxed(index), p)
    }
    fn fold_seq_len(&mut self, seq: Box<Expr>, p: Position) -> Expr {
        Expr::SeqLen(self.fold_boxed(seq), p)
    }
    fn fold_quantified_resource_access(&mut self, quant: QuantifiedResourceAccess, p: Position) -> Expr {
        Expr::QuantifiedResourceAccess(QuantifiedResourceAccess {
            vars: quant.vars,
            triggers: quant.triggers,
            cond: self.fold_boxed(quant.cond),
            resource: quant.resource.map_expression(|e| self.fold(e))
        }, p)
    }
}

pub fn default_fold_expr<T: ExprFolder>(this: &mut T, e: Expr) -> Expr {
    match e {
        Expr::Local(v, p) => this.fold_local(v, p),
        Expr::Variant(base, variant, p) => this.fold_variant(base, variant, p),
        Expr::Field(e, f, p) => this.fold_field(e, f, p),
        Expr::AddrOf(e, t, p) => this.fold_addr_of(e, t, p),
        Expr::Const(x, p) => this.fold_const(x, p),
        Expr::LabelledOld(x, y, p) => this.fold_labelled_old(x, y, p),
        Expr::MagicWand(x, y, b, p) => this.fold_magic_wand(x, y, b, p),
        Expr::PredicateAccessPredicate(x, y, z, p) => {
            this.fold_predicate_access_predicate(x, y, z, p)
        }
        Expr::FieldAccessPredicate(x, y, p) => this.fold_field_access_predicate(x, y, p),
        Expr::UnaryOp(x, y, p) => this.fold_unary_op(x, y, p),
        Expr::BinOp(x, y, z, p) => this.fold_bin_op(x, y, z, p),
        Expr::Unfolding(x, y, z, perm, variant, p) => {
            this.fold_unfolding(x, y, z, perm, variant, p)
        },
        Expr::Cond(x, y, z, p) => this.fold_cond(x, y, z, p),
        Expr::ForAll(x, y, z, p) => this.fold_forall(x, y, z, p),
        Expr::LetExpr(x, y, z, p) => this.fold_let_expr(x, y, z, p),
        Expr::FuncApp(x, y, z, k, p) => this.fold_func_app(x, y, z, k, p),
        Expr::SeqIndex(x, y, p) => this.fold_seq_index(x, y, p),
        Expr::SeqLen(x, p) => this.fold_seq_len(x, p),
        Expr::QuantifiedResourceAccess(x, p) => this.fold_quantified_resource_access(x, p),
    }
}

pub trait ExprWalker: Sized {
    fn walk(&mut self, expr: &Expr) {
        default_walk_expr(self, expr);
    }

    fn walk_local_var(&mut self, _var: &LocalVar) {}

    fn walk_local(&mut self, var: &LocalVar, _pos: &Position) {
        self.walk_local_var(var);
    }
    fn walk_variant(&mut self, base: &Expr, _variant: &Field, _pos: &Position) {
        self.walk(base);
    }
    fn walk_field(&mut self, receiver: &Expr, _field: &Field, _pos: &Position) {
        self.walk(receiver);
    }
    fn walk_addr_of(&mut self, receiver: &Expr, _typ: &Type, _pos: &Position) {
        self.walk(receiver);
    }
    fn walk_const(&mut self, _const: &Const, _pos: &Position) {}
    fn walk_labelled_old(&mut self, _label: &str, body: &Expr, _pos: &Position) {
        self.walk(body);
    }
    fn walk_magic_wand(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        _borrow: &Option<Borrow>,
        _pos: &Position
    ) {
        self.walk(lhs);
        self.walk(rhs);
    }
    fn walk_predicate_access_predicate(
        &mut self,
        _name: &str,
        arg: &Expr,
        _perm_amount: PermAmount,
        _pos: &Position
    ) {
        self.walk(arg)
    }
    fn walk_field_access_predicate(
        &mut self,
        receiver: &Expr,
        _perm_amount: PermAmount,
        _pos: &Position
    ) {
        self.walk(receiver)
    }
    fn walk_unary_op(&mut self, _op: UnaryOpKind, arg: &Expr, _pos: &Position) {
        self.walk(arg)
    }
    fn walk_bin_op(&mut self, _op: BinOpKind, arg1: &Expr, arg2: &Expr, _pos: &Position) {
        self.walk(arg1);
        self.walk(arg2);
    }
    fn walk_unfolding(
        &mut self,
        _name: &str,
        args: &Vec<Expr>,
        body: &Expr,
        _perm: PermAmount,
        _variant: &MaybeEnumVariantIndex,
        _pos: &Position
    ) {
        for arg in args {
            self.walk(arg);
        }
        self.walk(body);
    }
    fn walk_cond(&mut self, guard: &Expr, then_expr: &Expr, else_expr: &Expr, _pos: &Position) {
        self.walk(guard);
        self.walk(then_expr);
        self.walk(else_expr);
    }
    fn walk_forall(
        &mut self,
        vars: &Vec<LocalVar>,
        _triggers: &Vec<Trigger>,
        body: &Expr,
        _pos: &Position
    ) {
        for var in vars {
            self.walk_local_var(var);
        }
        self.walk(body);
    }
    fn walk_let_expr(&mut self, bound_var: &LocalVar, expr: &Expr, body: &Expr, _pos: &Position) {
        self.walk_local_var(bound_var);
        self.walk(expr);
        self.walk(body);
    }
    fn walk_func_app(
        &mut self,
        _name: &str,
        args: &Vec<Expr>,
        formal_args: &Vec<LocalVar>,
        _return_type: &Type,
        _pos: &Position
    ) {
        for arg in args {
            self.walk(arg)
        }
        for arg in formal_args {
            self.walk_local_var(arg);
        }
    }
    fn walk_seq_index(&mut self, base: &Expr, index: &Expr, _pos: &Position) {
        self.walk(base);
        self.walk(index);
    }
    fn walk_seq_len(&mut self, arg: &Expr, _pos: &Position) {
        self.walk(arg)
    }
    fn walk_quantified_resource_access(&mut self, quant: &QuantifiedResourceAccess, _pos: &Position) {
        for var in &quant.vars {
            self.walk_local_var(var);
        }
        self.walk(&*quant.cond);
        self.walk(quant.resource.get_place());
    }
}

pub fn default_walk_expr<T: ExprWalker>(this: &mut T, e: &Expr) {
    match *e {
        Expr::Local(ref v, ref p) => this.walk_local(v, p),
        Expr::Variant(ref base, ref variant, ref p) => this.walk_variant(base, variant, p),
        Expr::Field(ref e, ref f, ref p) => this.walk_field(e, f, p),
        Expr::AddrOf(ref e, ref t, ref p) => this.walk_addr_of(e, t, p),
        Expr::Const(ref x, ref p) => this.walk_const(x, p),
        Expr::LabelledOld(ref x, ref y, ref p) => this.walk_labelled_old(x, y, p),
        Expr::MagicWand(ref x, ref y, ref b, ref p) => this.walk_magic_wand(x, y, b, p),
        Expr::PredicateAccessPredicate(ref x, ref y, z, ref p) => {
            this.walk_predicate_access_predicate(x, y, z, p)
        }
        Expr::FieldAccessPredicate(ref x, y, ref p) => this.walk_field_access_predicate(x, y, p),
        Expr::UnaryOp(x, ref y, ref p) => this.walk_unary_op(x, y, p),
        Expr::BinOp(x, ref y, ref z, ref p) => this.walk_bin_op(x, y, z, p),
        Expr::Unfolding(ref x, ref y, ref z, perm, ref variant, ref p) => {
            this.walk_unfolding(x, y, z, perm, variant, p)
        },
        Expr::Cond(ref x, ref y, ref z, ref p) => this.walk_cond(x, y, z, p),
        Expr::ForAll(ref x, ref y, ref z, ref p) => this.walk_forall(x, y, z, p),
        Expr::LetExpr(ref x, ref y, ref z, ref p) => this.walk_let_expr(x, y, z, p),
        Expr::FuncApp(ref x, ref y, ref z, ref k, ref p) => this.walk_func_app(x, y, z, k, p),
        Expr::SeqIndex(ref x, ref y, ref p) => this.walk_seq_index(x, y, p),
        Expr::SeqLen(ref x, ref p) => this.walk_seq_len(x, p),
        Expr::QuantifiedResourceAccess(ref x, ref p) => this.walk_quantified_resource_access(x, p),
    }
}

impl Expr {
    /// Remove read permissions. For example, if the expression is
    /// `acc(x.f, read) && acc(P(x.f), write)`, then after the
    /// transformation it will be: `acc(P(x.f), write)`.
    pub fn remove_read_permissions(self) -> Self {
        struct ReadPermRemover {};
        impl ExprFolder for ReadPermRemover {
            fn fold_predicate_access_predicate(
                &mut self,
                name: String,
                arg: Box<Expr>,
                perm_amount: PermAmount,
                p: Position,
            ) -> Expr {
                assert!(perm_amount.is_valid_for_specs());
                match perm_amount {
                    PermAmount::Write => Expr::PredicateAccessPredicate(name, arg, perm_amount, p),
                    PermAmount::Read => true.into(),
                    _ => unreachable!(),
                }
            }
            fn fold_field_access_predicate(
                &mut self,
                reference: Box<Expr>,
                perm_amount: PermAmount,
                p: Position,
            ) -> Expr {
                assert!(perm_amount.is_valid_for_specs());
                match perm_amount {
                    PermAmount::Write => Expr::FieldAccessPredicate(reference, perm_amount, p),
                    PermAmount::Read => true.into(),
                    _ => unreachable!(),
                }
            }
        }
        let mut remover = ReadPermRemover {};
        remover.fold(self)
    }
}

#[derive(Debug, Clone)]
pub struct InstantiationResult {
    instantiated: QuantifiedResourceAccess,
    target_place_expr: Expr,
    match_type: InstantiationResultMatchType,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum InstantiationResultMatchType {
    PerfectFieldAccMatch,
    PerfectPredAccMatch,
    PrefixFieldAccMatch,
    PrefixPredAccMatch,
}

pub struct ProperPrefixResult {
    // TODO: not filled
    pub vars_mapping: HashMap<LocalVar, LocalVar>,
    // Whether the preconditions are syntactically the same (up to the names of the quantified variables)
    pub identical_cond: bool,
}

// TODO: very bad name
pub struct SimilarToResult {
    pub vars_mapping: HashMap<LocalVar, LocalVar>,
    // Whether the preconditions are syntactically the same (up to the names of the quantified variables)
    pub identical_cond: bool,
}

impl QuantifiedResourceAccess {
    pub fn try_instantiate(&self, perm_place: &Expr) -> Option<InstantiationResult> {
        if self.vars.is_empty() {
            self.try_instantiate_empty_vars(perm_place)
        } else {
            self.try_instantiate_non_empty_vars(perm_place)
        }
    }

    fn try_instantiate_empty_vars(&self, perm_place: &Expr) -> Option<InstantiationResult> {
        assert!(self.vars.is_empty());
        if !perm_place.has_prefix(self.resource.get_place()) {
            return None;
        }
        let match_type =
            match (perm_place == self.resource.get_place(), self.resource.is_field_acc()) {
                (true, true) => InstantiationResultMatchType::PerfectFieldAccMatch,
                (true, false) => InstantiationResultMatchType::PerfectPredAccMatch,
                (false, true) => InstantiationResultMatchType::PrefixFieldAccMatch,
                (false, false) => InstantiationResultMatchType::PrefixPredAccMatch,
            };
        Some(InstantiationResult::new(self.clone(), perm_place.clone(), match_type))
    }

    fn try_instantiate_non_empty_vars(&self, perm_place: &Expr) -> Option<InstantiationResult> {
        assert!(!self.vars.is_empty());
        let vars = self.vars.iter().cloned().collect();
        let forall_body = Expr::BinOp(
            BinOpKind::Implies,
            self.cond.clone(),
            box self.resource.to_expression(),
            Position::default()
        );
        forall_instantiation(perm_place, &vars, &self.triggers, &forall_body, false)
            .map(|fi| {
                let remaining_vars = self.vars.iter()
                    .filter(|&v| !fi.vars_mapping.contains_key(v))
                    .cloned()
                    .collect::<Vec<_>>();
                let substed_triggers = {
                    if remaining_vars.is_empty() {
                        Vec::new()
                    } else {
                        // TODO: filter out triggers that become "useless"
                        self.triggers.iter()
                            .map(|trigger| Trigger::new(
                                trigger.elements()
                                    .iter()
                                    .map(|e| e.clone().subst_vars(&fi.vars_mapping)).collect()
                            )).collect::<Vec<_>>()
                    }
                };

                match *fi.body {
                    Expr::BinOp(BinOpKind::Implies, cond, box resource, _) => {
                        match resource {
                            Expr::FieldAccessPredicate(field_place, perm, _) => {
                                let match_type = if &*field_place == perm_place {
                                    InstantiationResultMatchType::PerfectFieldAccMatch
                                } else {
                                    InstantiationResultMatchType::PrefixFieldAccMatch
                                };
                                let instantiated = QuantifiedResourceAccess {
                                    vars: remaining_vars,
                                    triggers: substed_triggers,
                                    cond,
                                    resource: PlainResourceAccess::Field(
                                        FieldAccessPredicate {
                                            place: field_place,
                                            perm
                                        }
                                    )
                                };
                                assert!(
                                    perm_place.has_prefix(instantiated.resource.get_place()),
                                    "{} does not have {} as a prefix", perm_place, instantiated.resource.get_place()
                                );
                                InstantiationResult::new(instantiated, perm_place.clone(), match_type)
                            }
                            Expr::PredicateAccessPredicate(predicate_name, pred_place, perm, _) => {
                                let match_type = if &*pred_place == perm_place {
                                    InstantiationResultMatchType::PerfectPredAccMatch
                                } else {
                                    InstantiationResultMatchType::PrefixPredAccMatch
                                };
                                let pred = PredicateAccessPredicate::new(*pred_place, perm)
                                    .expect("Ill-formed predicate instantiation");
                                assert_eq!(predicate_name, pred.predicate_name);
                                let instantiated = QuantifiedResourceAccess {
                                    vars: remaining_vars,
                                    triggers: substed_triggers,
                                    cond,
                                    resource: PlainResourceAccess::Predicate(pred)
                                };
                                assert!(perm_place.has_prefix(instantiated.resource.get_place()));
                                InstantiationResult::new(instantiated, perm_place.clone(), match_type)
                            }
                            x => unreachable!("forall_instantiation altered resource: {}", x),
                        }
                    }
                    x => unreachable!("We have given an implication, but forall_instantiation gave us back {}", x),
                }
            })
    }

    /// Check that two quantified resource accesses are *syntactically* the same
    /// (up to the names of the quantified variables).
    pub fn is_similar_to(&self, other: &QuantifiedResourceAccess, check_perm: bool) -> bool {
        unify(
            &Expr::QuantifiedResourceAccess(self.clone(), Position::default()),
            &Expr::QuantifiedResourceAccess(other.clone(), Position::default()),
            &HashSet::new(),
            &mut HashMap::new(),
            check_perm
        ).is_success()
    }

    /// Like ```is_similar_to``` but does not take the preconditions in account.
    pub fn is_similar_to_ignoring_preconds(
        &self,
        other: &QuantifiedResourceAccess,
        check_perm: bool
    ) -> Option<SimilarToResult> {
        if self.vars.len() != other.vars.len() {
            // We assume that all vars are used...
            return None;
        }
        let mut vars_mapping = HashMap::new();
        if unify(
            &self.resource.to_expression(),
            &other.resource.to_expression(),
            // The free vars asked by unify is for the subject (here, self)
            &self.vars.iter().cloned().collect(),
            &mut vars_mapping,
            check_perm
        ).is_success() {
            let vars_mapping_lvs = vars_mapping.into_iter()
                .filter_map(|(lhs_lv, rhs_expr)| match rhs_expr {
                    Expr::Local(rhs_lv, _) => Some((lhs_lv, rhs_lv)),
                    _ => None
                }).collect::<HashMap<LocalVar, LocalVar>>();
            if vars_mapping_lvs.len() != self.vars.len() {
                None
            } else {
                let identical_cond = *self.cond == other.cond.clone().rename(&vars_mapping_lvs);
                Some(SimilarToResult {
                    vars_mapping: vars_mapping_lvs,
                    identical_cond
                })
            }
        } else {
            None
        }
    }

    pub fn has_proper_prefix(&self, other: &QuantifiedResourceAccess) -> Option<ProperPrefixResult> {
        if self.vars.len() != other.vars.len() {
            // We assume that all vars are used...
            return None;
        }
        // FIXME: do this correctly by unifying the bounded vars
        if self.resource.get_place().has_proper_prefix(other.resource.get_place()) {
            Some(ProperPrefixResult {
                vars_mapping: HashMap::new(), // TODO
                identical_cond: self.cond == other.cond // TODO: do not forget to rename these according to vars_mapping
            })
        } else {
            None
        }
    }

    pub fn to_forall_expression(&self) -> Expr {
        let body = Expr::BinOp(
            BinOpKind::Implies,
            self.cond.clone(),
            box self.resource.to_expression(),
            Position::default()
        );
        if self.vars.is_empty() {
            body
        } else {
            Expr::ForAll(self.vars.clone(), self.triggers.clone(), box body, Position::default())
        }
    }

    pub fn get_perm_amount(&self) -> PermAmount {
        self.resource.get_perm_amount()
    }

    pub fn update_perm_amount(self, new_perm: PermAmount) -> Self {
        QuantifiedResourceAccess {
            vars: self.vars,
            triggers: self.triggers,
            cond: self.cond,
            resource: self.resource.update_perm_amount(new_perm)
        }
    }

    // TODO: misleading name
    pub fn map_place<F>(self, f: F) -> Self
        where F: Fn(Expr) -> Expr
    {
        let triggers = self.triggers.into_iter()
            .map(|trigger| trigger.map_all(&f))
            .collect();
        QuantifiedResourceAccess {
            vars: self.vars,
            triggers,
            cond: box f(*self.cond),
            resource: self.resource.map_expression(f)
        }
    }
}

impl InstantiationResult {
    fn new(instantiated: QuantifiedResourceAccess,
           target_place_expr: Expr,
           match_type: InstantiationResultMatchType) -> Self {
        // Sanity check
        assert!(target_place_expr.has_prefix(instantiated.resource.get_place()));
        InstantiationResult { instantiated, target_place_expr, match_type }
    }

    pub fn instantiated(&self) -> &QuantifiedResourceAccess {
        &self.instantiated
    }

    // TODO: bad name
    pub fn into_instantiated(self) -> QuantifiedResourceAccess {
        self.instantiated
    }

    pub fn is_fully_instantiated(&self) -> bool {
        self.instantiated.vars.is_empty()
    }

    pub fn match_type(&self) -> InstantiationResultMatchType {
        self.match_type
    }

    pub fn is_match_perfect(&self) -> bool {
        match self.match_type {
            InstantiationResultMatchType::PerfectFieldAccMatch
                | InstantiationResultMatchType::PerfectPredAccMatch => true,
            InstantiationResultMatchType::PrefixFieldAccMatch
                | InstantiationResultMatchType::PrefixPredAccMatch => false,
        }
    }
}

impl PlainResourceAccess {
    pub fn field(place: Expr, perm: PermAmount) -> Self {
        PlainResourceAccess::Field(FieldAccessPredicate {
            place: box place,
            perm,
        })
    }

    pub fn predicate(place: Expr, perm: PermAmount) -> Option<Self> {
        PredicateAccessPredicate::new(place, perm).map(PlainResourceAccess::Predicate)
    }

    pub fn get_place(&self) -> &Expr {
        match self {
            PlainResourceAccess::Predicate(p) => &*p.arg,
            PlainResourceAccess::Field(f) => &*f.place,
        }
    }

    pub fn into_place(self) -> Expr {
        match self {
            PlainResourceAccess::Predicate(p) => *p.arg,
            PlainResourceAccess::Field(f) => *f.place,
        }
    }

    pub fn is_pred(&self) -> bool {
        match self {
            PlainResourceAccess::Predicate(_) => true,
            _ => false,
        }
    }

    pub fn is_field_acc(&self) -> bool {
        match self {
            PlainResourceAccess::Field(_) => true,
            _ => false,
        }
    }

    pub fn to_expression(&self) -> Expr {
        self.clone().into()
    }

    pub fn get_perm_amount(&self) -> PermAmount {
        match self {
            PlainResourceAccess::Predicate(p) => p.perm,
            PlainResourceAccess::Field(f) => f.perm,
        }
    }

    pub fn update_perm_amount(self, new_perm: PermAmount) -> Self {
        match self {
            PlainResourceAccess::Predicate(p) =>
                PlainResourceAccess::Predicate(PredicateAccessPredicate {
                    predicate_name: p.predicate_name,
                    arg: p.arg,
                    perm: new_perm
                }),
            PlainResourceAccess::Field(f) =>
                PlainResourceAccess::Field(FieldAccessPredicate {
                    place: f.place,
                    perm: new_perm
                }),
        }
    }

    pub fn map_expression<F>(self, f: F) -> Self
        where F: FnOnce(Expr) -> Expr
    {
        match self {
            PlainResourceAccess::Predicate(pa) =>
                PlainResourceAccess::Predicate(PredicateAccessPredicate {
                    predicate_name: pa.predicate_name,
                    arg: box f(*pa.arg),
                    perm: pa.perm
                }),
            PlainResourceAccess::Field(fa) =>
                PlainResourceAccess::Field(FieldAccessPredicate {
                    place: box f(*fa.place),
                    perm: PermAmount::Read,
                }),
        }
    }
}

impl PredicateAccessPredicate {
    pub fn new(place: Expr, perm: PermAmount) -> Option<Self> {
        place.typed_ref_name()
            .map(|pred_name|
                PredicateAccessPredicate {
                    predicate_name: pred_name,
                    arg: box place,
                    perm
                }
            )
    }
}

impl Into<Expr> for PlainResourceAccess {
    fn into(self) -> Expr {
        match self {
            PlainResourceAccess::Predicate(p) =>
                Expr::PredicateAccessPredicate(p.predicate_name, p.arg, p.perm, Position::default()),
            PlainResourceAccess::Field(f) =>
                Expr::FieldAccessPredicate(f.place, f.perm, Position::default()),
        }
    }
}

impl Into<QuantifiedResourceAccess> for InstantiationResult {
    fn into(self) -> QuantifiedResourceAccess {
        self.instantiated
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
struct ForallInstantiation {
    vars_mapping: HashMap<LocalVar, Expr>,
    body: Box<Expr>,
}

#[derive(Debug, Clone)]
struct ForallInstantiations(Vec<ForallInstantiation>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnificationResult {
    Success,
    Unmatched,
    Conflict,
}

impl UnificationResult {
    fn is_success(&self) -> bool {
        match self {
            UnificationResult::Success => true,
            _ => false,
        }
    }
}

fn unify(
    subject: &Expr,
    target: &Expr,
    free_vars: &HashSet<LocalVar>,
    vars_mapping: &mut HashMap<LocalVar, Expr>,
    check_perms: bool,
) -> UnificationResult {
    fn do_unify(
        subject: &Expr,
        target: &Expr,
        free_vars: &HashSet<LocalVar>,
        // The original mapping that we were passed.
        // We will modify it at the end once we are sure the unification succeeded
        orig_mapping: &HashMap<LocalVar, Expr>,
        vars_mapping: &mut HashMap<LocalVar, Expr>,
        check_perms: bool,
    ) -> Result<(), UnificationResult> { // We return Result for the `?` operator convenience
        match (subject, target) {
            (Expr::Local(lv, _), _) if free_vars.contains(lv) => {
                match vars_mapping.entry(lv.clone()) {
                    Entry::Vacant(e) => {
                        e.insert(target.clone());
                        Ok(())
                    }
                    Entry::Occupied(e) => {
                        // We already have registered a mapping for this free var.
                        // If the expressions don't correspond, the unification failed.
                        // This can arise if we have e.g.:
                        // target = f(v, v)  with fv = {v}
                        // subject = f(5, 19)
                        // In that case, we can't unify the expression so we return UnificationResult::Conflict.
                        if &*e.get() == target {
                            // Do the same for the original mapping
                            if let Some(expr_in_original) = orig_mapping.get(&lv) {
                                if e.get() == expr_in_original {
                                    Ok(())
                                } else {
                                    Err(UnificationResult::Conflict)
                                }
                            } else {
                                Ok(())
                            }
                        } else {
                            Err(UnificationResult::Conflict)
                        }
                    }
                }
            }

            (Expr::Local(rlv, _), Expr::Local(llv, _)) =>
                if rlv == llv { Ok(()) } else { Err(UnificationResult::Unmatched) },

            (Expr::Variant(lbase, lfield, _), Expr::Variant(rbase, rfield, _)) if lfield == rfield =>
                do_unify(lbase, rbase, free_vars, orig_mapping, vars_mapping, check_perms),

            (Expr::Field(lbase, lfield, _), Expr::Field(rbase, rfield, _)) if lfield == rfield =>
                do_unify(lbase, rbase, free_vars, orig_mapping, vars_mapping, check_perms),

            (Expr::AddrOf(lbase, lty, _), Expr::AddrOf(rbase, rty, _)) if lty == rty =>
                do_unify(lbase, rbase, free_vars, orig_mapping, vars_mapping, check_perms),

            (Expr::LabelledOld(llabel, lbase, _), Expr::LabelledOld(rlabel, rbase, _)) if llabel == rlabel =>
                do_unify(lbase, rbase, free_vars, orig_mapping, vars_mapping, check_perms),

            (Expr::Const(lconst, _), Expr::Const(rconst, _)) =>
                if lconst == rconst { Ok(()) } else { Err(UnificationResult::Unmatched) },

            // Not sure about this one
            (Expr::MagicWand(llhs, lrhs, lborrow, _), Expr::MagicWand(rlhs, rrhs, rborrow, _)) if lborrow == rborrow => {
                do_unify(llhs, rlhs, free_vars, orig_mapping, vars_mapping, check_perms)?;
                do_unify(lrhs, rrhs, free_vars, orig_mapping, vars_mapping, check_perms)
            }

            (
                Expr::PredicateAccessPredicate(lname, larg, lperm, _),
                Expr::PredicateAccessPredicate(rname, rarg, rperm, _)
            ) if (!check_perms || lperm == rperm) && lname == rname =>
                do_unify(larg, rarg, free_vars, orig_mapping, vars_mapping, check_perms),

            (
                Expr::FieldAccessPredicate(larg, lperm, _),
                Expr::FieldAccessPredicate(rarg, rperm, _)
            ) if !check_perms || lperm == rperm =>
                do_unify(larg, rarg, free_vars, orig_mapping, vars_mapping, check_perms),

            (Expr::UnaryOp(lop, larg, _), Expr::UnaryOp(rop, rarg, _)) if lop == rop =>
                do_unify(larg, rarg, free_vars, orig_mapping, vars_mapping, check_perms),

            (Expr::BinOp(lop, larg1, larg2, _), Expr::BinOp(rop, rarg1, rarg2, _)) if lop == rop => {
                do_unify(larg1, rarg1, free_vars, orig_mapping, vars_mapping, check_perms)?;
                do_unify(larg2, rarg2, free_vars, orig_mapping, vars_mapping, check_perms)
            }

            (
                Expr::Unfolding(lname, largs, lin_expr, lperm, lenum, _),
                Expr::Unfolding(rname, rargs, rin_expr, rperm, renum, _)
            ) if lname == rname && lperm == rperm && lenum == renum => {
                if largs.len() != rargs.len() {
                    return Err(UnificationResult::Unmatched);
                }

                largs.iter()
                    .zip(rargs.iter())
                    .try_fold((), |(), (larg, rarg)|
                        do_unify(larg, rarg, free_vars, orig_mapping, vars_mapping, check_perms)
                    )?;
                do_unify(lin_expr, rin_expr, free_vars, orig_mapping, vars_mapping, check_perms)
            }

            (Expr::Cond(lguard, lthen, lelse, _), Expr::Cond(rguard, rthen, relse, _)) => {
                do_unify(lguard, rguard, free_vars, orig_mapping, vars_mapping, check_perms)?;
                do_unify(lthen, rthen, free_vars, orig_mapping, vars_mapping, check_perms)?;
                do_unify(lelse, relse, free_vars, orig_mapping, vars_mapping, check_perms)
            }

            (
                Expr::ForAll(lvars, _, lbody, _),
                Expr::ForAll(rvars, _, rbody, _)
            ) if lvars.len() == rvars.len() => {
                let mut new_free_vars = free_vars.clone();
                new_free_vars.extend(lvars.iter().cloned());
                // Implementation limitation: we do not support renaming
                assert_eq!(new_free_vars.len(), free_vars.len() + lvars.len());

                // TODO: unify triggers too!

                do_unify(lbody, rbody, &new_free_vars, orig_mapping, vars_mapping, check_perms)?;
                let mut matched_rvars = HashSet::new();
                for lv in lvars {
                    match vars_mapping.remove(lv) {
                        Some(Expr::Local(rv, _)) => {
                            if !matched_rvars.insert(rv) {
                                // Matched to the same variable more than once
                                return Err(UnificationResult::Unmatched);
                            }
                        }
                        Some(_) =>
                            // Matched to something other than the variables of the rhs forall
                            return Err(UnificationResult::Unmatched),
                        None => (), // The variable was unused
                    }
                }
                Ok(())
            }

            (Expr::LetExpr(lvar, lexpr, lbody, _), Expr::LetExpr(rvar, rexpr, rbody, _)) if lvar.typ == rvar.typ => {
                do_unify(lexpr, rexpr, free_vars, orig_mapping, vars_mapping, check_perms)?;

                let mut lnewbody: Option<Box<Expr>> = None;
                let mut rnewbody: Option<Box<Expr>> = None;
                if lvar != rvar {
                    // We need to rename things out
                    let common_name = "__".to_owned() + &lvar.name + "$" + &rvar.name + "__";
                    let newvar = LocalVar::new(common_name, lvar.typ.clone());
                    lnewbody = Some(box lbody.clone().rename_single(lvar, newvar.clone()));
                    rnewbody = Some(box rbody.clone().rename_single(rvar, newvar.clone()));
                    assert!(!free_vars.contains(&newvar));
                }
                // Get the renamed bodies, or the original one if we don't need renaming
                let (lbody, rbody) = match (&lnewbody, &rnewbody) {
                    (Some(l), Some(r)) => (l, r),
                    _ => (lbody, rbody)
                };
                do_unify(lbody, rbody, free_vars, orig_mapping, vars_mapping, check_perms)
            }

            (
                Expr::FuncApp(lname, largs, lformal_args, lrettype, _),
                Expr::FuncApp(rname, rargs, rformal_args, rrettype, _)
            ) if lname == rname && lformal_args == rformal_args && lrettype == rrettype => { // TODO: is comparing the formal arguments a bit too restrictive?
                assert_eq!(largs.len(), rargs.len());

                largs.iter()
                    .zip(rargs.iter())
                    .try_fold((), |(), (larg, rarg)|
                        do_unify(larg, rarg, free_vars, orig_mapping, vars_mapping, check_perms)
                    )
            }

            (Expr::SeqIndex(lseq, lindex, _), Expr::SeqIndex(rseq, rindex, _)) => {
                do_unify(lseq, rseq, free_vars, orig_mapping, vars_mapping, check_perms)?;
                do_unify(lindex, rindex, free_vars, orig_mapping, vars_mapping, check_perms)
            }

            (Expr::SeqLen(lseq, _), Expr::SeqLen(rseq, _)) =>
                do_unify(lseq, rseq, free_vars, orig_mapping, vars_mapping, check_perms),

            (Expr::QuantifiedResourceAccess(lquant, _), Expr::QuantifiedResourceAccess(rquant, _)) =>
                do_unify(
                    &lquant.to_forall_expression(),
                    &rquant.to_forall_expression(),
                    free_vars,
                    orig_mapping,
                    vars_mapping,
                    check_perms
                ),

            _ => Err(UnificationResult::Unmatched),
        }
    }

    let mut temp_mapping = HashMap::new();
    match do_unify(subject, target, free_vars, vars_mapping, &mut temp_mapping, check_perms) {
        Ok(()) => {
            vars_mapping.extend(temp_mapping);
            UnificationResult::Success
        }
        Err(e) => e
    }
}

fn forall_instantiation(
    target: &Expr,
    // forall params: vars, triggers and its body
    vars: &HashSet<LocalVar>,
    triggers: &Vec<Trigger>,
    body: &Expr,
    check_perms: bool,
) -> Option<ForallInstantiation> {
    fn inner(
        target: &Expr,
        vars: &HashSet<LocalVar>,
        trigger: &Vec<Expr>,
        matched_trigger: &mut Vec<bool>,
        vars_mapping: &mut HashMap<LocalVar, Expr>,
        check_perms: bool,
    ) -> Result<(), ()> { // Ok -> may or may not have matched all trigger. Err -> unification conflict
        let target_depth = target.depth();
        for (trigger, matched) in trigger.iter().zip(matched_trigger.iter_mut()) {
            let trigger_depth = trigger.depth();

            if *matched || trigger_depth > target_depth {
                continue;
            } else {
                match unify(trigger, target, vars, vars_mapping, check_perms) {
                    UnificationResult::Success => *matched = true,
                    UnificationResult::Unmatched => (),
                    UnificationResult::Conflict => return Err(()),
                };
            }
        }

        if matched_trigger.iter().all(|b| *b) {
            return Ok(());
        }

        match target {
            Expr::Local(_, _) =>
                Ok(()), // Nothing to do

            Expr::Variant(base, _, _) =>
                inner(base, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::Field(base, _, _) =>
                inner(base, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::AddrOf(base, _, _) =>
                inner(base, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::LabelledOld(_, base, _) =>
                inner(base, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::Const(_, _) =>
                Ok(()), // Nothing to do

            Expr::MagicWand(lhs, rhs, _, _) => {
                inner(lhs, vars, trigger, matched_trigger, vars_mapping, check_perms)?;
                inner(rhs, vars, trigger, matched_trigger, vars_mapping, check_perms)
            }

            Expr::PredicateAccessPredicate(_, arg, _, _) =>
                inner(arg, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::FieldAccessPredicate(arg, _, _) =>
                inner(arg, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::UnaryOp(_, arg, _) =>
                inner(arg, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::BinOp(_, lhs, rhs, _) => {
                inner(lhs, vars, trigger, matched_trigger, vars_mapping, check_perms)?;
                inner(rhs, vars, trigger, matched_trigger, vars_mapping, check_perms)
            }

            Expr::Unfolding(_, predicate_args, in_expr, _, _, _) => {
                predicate_args.iter()
                    .try_for_each(|arg|
                        inner(arg, vars, trigger, matched_trigger, vars_mapping, check_perms)
                    )?;
                inner(in_expr, vars, trigger, matched_trigger, vars_mapping, check_perms)
            }

            Expr::Cond(guard, then_expr, else_expr, _) => {
                inner(guard, vars, trigger, matched_trigger, vars_mapping, check_perms)?;
                inner(then_expr, vars, trigger, matched_trigger, vars_mapping, check_perms)?;
                inner(else_expr, vars, trigger, matched_trigger, vars_mapping, check_perms)
            }

            Expr::ForAll(..) => unimplemented!("Nested foralls are unsupported for now"),

            // TODO: we should remove the let variable from the free vars
            Expr::LetExpr(_, defexpr, body, _) => {
                inner(defexpr, vars, trigger, matched_trigger, vars_mapping, check_perms)?;
                inner(body, vars, trigger, matched_trigger, vars_mapping, check_perms)
            }

            Expr::FuncApp(_, args, _, _, _) => {
                args.iter()
                    .try_for_each(|arg|
                        inner(arg, vars, trigger, matched_trigger, vars_mapping, check_perms)
                    )
            }

            Expr::SeqIndex(seq, index, _) => {
                inner(seq, vars, trigger, matched_trigger, vars_mapping, check_perms)?;
                inner(index, vars, trigger, matched_trigger, vars_mapping, check_perms)
            }

            Expr::SeqLen(seq, _) =>
                inner(seq, vars, trigger, matched_trigger, vars_mapping, check_perms),

            Expr::QuantifiedResourceAccess(..) =>
                unimplemented!("QuantifiedResourceAccess are unsupported for now"),
        }
    }

    let mut vars_mapping = HashMap::new();
    let mut matched_trigger = Vec::new();
    // TODO: that's not idiomatic Rust
    for trigger in triggers {
        matched_trigger.resize(trigger.elements().len(), false);
        matched_trigger.iter_mut().for_each(|b| *b = false);
        vars_mapping.clear();

        if inner(target, vars, trigger.elements(), &mut matched_trigger, &mut vars_mapping, check_perms).is_ok()
         && matched_trigger.iter().all(|b| *b)
        {
            let subst_map = vars_mapping.iter()
                .map(|(lv, e)| (Expr::local(lv.clone()), (&*e).clone()))
                .collect::<HashMap<Expr, Expr>>();
            let substed_body = body.clone().subst(&subst_map);
            return Some(ForallInstantiation {
                vars_mapping,
                body: box substed_body,
            });
        }
    }
    None
}

pub trait ExprIterator {
    /// Conjoin a sequence of expressions into a single expression.
    /// Returns true if the sequence has no elements.
    fn conjoin(&mut self) -> Expr;

    /// Disjoin a sequence of expressions into a single expression.
    /// Returns true if the sequence has no elements.
    fn disjoin(&mut self) -> Expr;
}

impl<T> ExprIterator for T
where
    T: Iterator<Item = Expr>,
{
    fn conjoin(&mut self) -> Expr {
        fn rfold<T>(s: &mut T) -> Expr
        where
            T: Iterator<Item = Expr>,
        {
            if let Some(conjunct) = s.next() {
                Expr::and(conjunct, rfold(s))
            } else {
                true.into()
            }
        }
        rfold(self)
    }

    fn disjoin(&mut self) -> Expr {
        fn rfold<T>(s: &mut T) -> Expr
        where
            T: Iterator<Item = Expr>,
        {
            if let Some(conjunct) = s.next() {
                Expr::or(conjunct, rfold(s))
            } else {
                false.into()
            }
        }
        rfold(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoder::vir::Const::Int;

// TODO: test renaming of let variables & cie.

    #[test]
    fn test_unify_success_simple() {
        let x = Expr::local(LocalVar::new("x", Type::Int));
        let y = Expr::local(LocalVar::new("y", Type::Int));
        let z = Expr::local(LocalVar::new("z", Type::Int));
        let a = Expr::local(LocalVar::new("a", Type::Int));
        let b = Expr::local(LocalVar::new("b", Type::Int));
        let c = Expr::local(LocalVar::new("c", Type::Int));
        let fv1 = LocalVar::new("fv1", Type::Int);
        let fv2 = LocalVar::new("fv2", Type::Int);

        let expr_builder = |a: &Expr, b: &Expr| -> Expr {
            Expr::BinOp(
                BinOpKind::Mul,
                box Expr::BinOp(
                    BinOpKind::Add,
                    box x.clone(),
                    box Expr::BinOp(
                        BinOpKind::Div,
                        box y.clone(),
                        box Expr::Cond(
                            box a.clone(),
                            box z.clone(),
                            box b.clone(),
                            Position::default()
                        ),
                        Position::default()
                    ),
                    Position::default(),
                ),
                box a.clone(),
                Position::default(),
            )
        };
        // (x + (y / (fv1subst ? z : fv2subst))) * fv1subst
        let subject = expr_builder(&Expr::local(fv1.clone()), &Expr::local(fv2.clone()));
        let fv1subst = Expr::BinOp(BinOpKind::Add, box z.clone(), box a.clone(), Position::default());
        let fv2subst = Expr::BinOp(BinOpKind::Mul, box b.clone(), box c.clone(), Position::default());
        // (x + (y / (fv1subst ? z : fv2subst))) * fv1subst
        let target = expr_builder(&fv1subst, &fv2subst);
        let mut fvs = HashSet::new();
        fvs.insert(fv1.clone());
        fvs.insert(fv2.clone());
        let mut got = HashMap::new();
        let ok = unify(&subject, &target, &fvs, &mut got, false);
        assert_eq!(UnificationResult::Success, ok);

        let mut expected = HashMap::new();
        expected.insert(fv1, fv1subst);
        expected.insert(fv2, fv2subst);
        assert_eq!(expected, got);
    }

    #[test]
    fn test_unify_success_call() {
        let x = Expr::local(LocalVar::new("x", Type::Int));
        let y = Expr::local(LocalVar::new("y", Type::Int));
        let z = Expr::local(LocalVar::new("z", Type::Int));
        let a = Expr::local(LocalVar::new("a", Type::Int));
        let b = Expr::local(LocalVar::new("b", Type::Int));
        let c = Expr::local(LocalVar::new("c", Type::Int));
        let fv1 = LocalVar::new("fv1", Type::Int);
        let fv2 = LocalVar::new("fv2", Type::Int);

        // x + f(a, b, y * b)
        let expr_builder = |a: &Expr, b: &Expr| {
            Expr::BinOp(
                BinOpKind::Add,
                box x.clone(),
                box Expr::FuncApp(
                    "f".to_owned(),
                    vec![
                        a.clone(),
                        b.clone(),
                        Expr::BinOp(
                            BinOpKind::Mul,
                            box y.clone(),
                            box b.clone(),
                            Position::default()
                        )
                    ],
                    vec![
                        LocalVar::new("arg1", Type::Int),
                        LocalVar::new("arg2", Type::Int),
                        LocalVar::new("arg3", Type::Int),
                    ],
                    Type::Int,
                    Position::default()
                ),
                Position::default()
            )
        };
        // x + f(fv1, fv2, y * fv2)
        let subject = expr_builder(&Expr::local(fv1.clone()), &Expr::local(fv2.clone()));

        let fv1subst = Expr::BinOp(BinOpKind::Add, box z.clone(), box a.clone(), Position::default());
        let fv2subst = Expr::BinOp(BinOpKind::Mul, box b.clone(), box c.clone(), Position::default());
        // x + f(fv1subst, fv2subst, y * fv2subst)
        let target = expr_builder(&fv1subst, &fv2subst);

        let mut fvs = HashSet::new();
        fvs.insert(fv1.clone());
        fvs.insert(fv2.clone());
        let mut got = HashMap::new();
        let ok = unify(&subject, &target, &fvs, &mut got, false);
        assert_eq!(UnificationResult::Success, ok);

        let mut expected = HashMap::new();
        expected.insert(fv1, fv1subst);
        expected.insert(fv2, fv2subst);
        assert_eq!(expected, got);
    }

    #[test]
    fn test_unify_success_forall_simple() {
        let x = Expr::local(LocalVar::new("x", Type::Int));
        let z = Expr::local(LocalVar::new("z", Type::Int));
        let a = Expr::local(LocalVar::new("a", Type::Int));
        let b = Expr::local(LocalVar::new("b", Type::Int));
        let c = Expr::local(LocalVar::new("c", Type::Int));

        // i, j: forall vars
        // ph1, ph2: placeholders
        let forall_builder = |i: &LocalVar, j: &LocalVar, ph1: &Expr, ph2: &Expr, vars_in_order: bool| {
            let vars = if vars_in_order {
                vec![i.clone(), j.clone()]
            } else {
                vec![j.clone(), i.clone()]
            };
            // (x + (j / (ph1 ? i : ph2))) * ph1
            Expr::ForAll(vars, vec![],
                 box Expr::BinOp(
                     BinOpKind::Mul,
                     box Expr::BinOp(
                         BinOpKind::Add,
                         box x.clone(),
                         box Expr::BinOp(
                             BinOpKind::Div,
                             box Expr::local(j.clone()),
                             box Expr::Cond(box ph1.clone(), box Expr::local(i.clone()), box ph2.clone(), Position::default()),
                             Position::default()
                         ),
                         Position::default(),
                     ),
                     box ph1.clone(),
                     Position::default(),
                 ),
                 Position::default()
            )
        };

        let subject_i = LocalVar::new("i", Type::Int);
        let subject_j = LocalVar::new("j", Type::Int);
        let fv1 = LocalVar::new("fv1", Type::Int);
        let fv2 = LocalVar::new("fv2", Type::Int);
        let fv1_expr = Expr::local(fv1.clone());
        let fv2_expr = Expr::local(fv2.clone());
        let subject = forall_builder(&subject_i, &subject_j, &fv1_expr, &fv2_expr, true);

        let target_i = LocalVar::new("i", Type::Int);
        let target_j = LocalVar::new("j", Type::Int);
        let fv1subst = Expr::BinOp(BinOpKind::Add, box z.clone(), box a.clone(), Position::default());
        let fv2subst = Expr::BinOp(BinOpKind::Mul, box b.clone(), box c.clone(), Position::default());
        let target = forall_builder(&target_i, &target_j, &fv1subst, &fv2subst, false);

        let mut fvs = HashSet::new();
        fvs.insert(fv1.clone());
        fvs.insert(fv2.clone());
        let mut got = HashMap::new();
        let ok = unify(&subject, &target, &fvs, &mut got, false);
        assert_eq!(UnificationResult::Success, ok);

        let mut expected = HashMap::new();
        expected.insert(fv1, fv1subst);
        expected.insert(fv2, fv2subst);
        assert_eq!(expected, got);
    }

    #[test]
    fn test_unify_failure_simple() {
        let y = Expr::local(LocalVar::new("y", Type::Int));
        let z = Expr::local(LocalVar::new("z", Type::Int));
        let a = Expr::local(LocalVar::new("a", Type::Int));
        let b = Expr::local(LocalVar::new("b", Type::Int));
        let fv1 = LocalVar::new("fv1", Type::Int);

        let subject = Expr::BinOp(
            BinOpKind::Mul,
            box Expr::BinOp(
                BinOpKind::Add,
                box Expr::local(fv1.clone()),
                box Expr::BinOp(
                    BinOpKind::Div,
                    box y.clone(),
                    box z.clone(),
                    Position::default()
                ),
                Position::default(),
            ),
            box Expr::local(fv1.clone()),
            Position::default(),
        );
        let target = Expr::BinOp(
            BinOpKind::Mul,
            box Expr::BinOp(
                BinOpKind::Add,
                box a,
                box Expr::BinOp(
                    BinOpKind::Div,
                    box y.clone(),
                    box z.clone(),
                    Position::default()
                ),
                Position::default(),
            ),
            box b,
            Position::default(),
        );
        let mut fvs = HashSet::new();
        fvs.insert(fv1.clone());
        let mut got = HashMap::new();
        let ok = unify(&subject, &target, &fvs, &mut got, false);
        assert_eq!(UnificationResult::Conflict, ok);
        assert!(got.is_empty()); // Must be unchanged
    }

    #[test]
    fn test_forall_instantiation_simple() {
        // From http://viper.ethz.ch/tutorial/

        let magic_call = |arg: Expr| {
            Expr::FuncApp(
                "magic".into(),
                vec![arg],
                vec![LocalVar::new("magic_arg", Type::Int)],
                Type::Int,
                Position::default(),
            )
        };
        // magic(magic(arg)) == magic(2 * arg) + arg
        let magic_property_body = |arg: Expr| {
            Expr::BinOp(
                BinOpKind::EqCmp,
                box magic_call(magic_call(arg.clone())),
                box Expr::BinOp(
                    BinOpKind::Add,
                    box magic_call(
                        Expr::BinOp(
                            BinOpKind::Mul,
                            box Expr::Const(Const::Int(2), Position::default()),
                            box arg.clone(),
                            Position::default(),
                        )
                    ),
                    box arg,
                    Position::default(),
                ),
                Position::default(),
            )
        };

        // forall i: Int :: { magic(magic(i)) }
        //      magic(magic(i)) == magic(2 * i) + i
        let (forall_vars, forall_triggers, forall_body) = {
            let i = LocalVar::new("i", Type::Int);
            let triggers = vec![Trigger::new(vec![magic_call(magic_call(Expr::local(i.clone())))])];
            let body = magic_property_body(Expr::local(i.clone()));
            let mut vars = HashSet::new();
            vars.insert(i);
            (vars, triggers, body)
        };

        // 1
        {
            // magic(magic(10)) == magic(2 * 10) + 10
            let expr = magic_property_body(Expr::Const(Const::Int(10), Position::default()));
            let got = forall_instantiation(&expr, &forall_vars, &forall_triggers, &forall_body, false);
            let expected = {
                let mut mapping = HashMap::new();
                mapping.insert(LocalVar::new("i", Type::Int), Expr::Const(Const::Int(10), Position::default()));
                ForallInstantiation {
                    vars_mapping: mapping,
                    body: box expr // We get back the same expression for that specific example
                }
            };
            assert_eq!(Some(expected), got);
        }

        // 2
        {
            let x = Expr::local(LocalVar::new("x", Type::Int));
            let y = Expr::local(LocalVar::new("y", Type::Int));
            let z = Expr::local(LocalVar::new("z", Type::Int));
            let x_plus_z = Expr::BinOp(BinOpKind::Add, box x.clone(), box z.clone(), Position::default());

            // 42 + magic(magic(magic(x+z))) * y
            let expr = Expr::BinOp(
                BinOpKind::Add,
                box Expr::Const(Const::Int(42), Position::default()),
                box Expr::BinOp(
                    BinOpKind::Mul,
                    box magic_call(magic_call(magic_call(x_plus_z.clone()))),
                    box y,
                    Position::default()
                ),
                Position::default()
            );
            // Mapping: i -> magic(x+z)
            // Forall body: magic(magic(magic(x+z))) == magic(2 * magic(x+z)) + magic(x+z)
            let expected = {
                let magic_x_plus_z = magic_call(x_plus_z.clone());
                let mut mapping = HashMap::new();
                mapping.insert(LocalVar::new("i", Type::Int), magic_x_plus_z.clone());
                let body = magic_property_body(magic_x_plus_z);
                ForallInstantiation {
                    vars_mapping: mapping,
                    body: box body
                }
            };
            let got = forall_instantiation(&expr, &forall_vars, &forall_triggers, &forall_body, false);
            assert_eq!(Some(expected), got);
        }
    }

    #[test]
    fn test_quant_resource_access_similarity_success_simple_1() {
        let common_base = Expr::local(LocalVar::new("base", Type::TypedRef("t0".into())));
        let quant1_i = LocalVar::new("a", Type::Int);
        let quant1_j = LocalVar::new("b", Type::Int);
        let quant2_i = LocalVar::new("i", Type::Int);
        let quant2_j = LocalVar::new("j", Type::Int);
        let quant1 = quant_resource_builder_sample_1(&quant1_i, &quant1_j, &common_base, true);
        let quant2 = quant_resource_builder_sample_1(&quant2_i, &quant2_j, &common_base, false);
        // Two quant. access that are similar except in the names of the bound vars.
        assert!(quant1.is_similar_to(&quant2, false));
    }

    #[test]
    fn test_quant_resource_access_similarity_success_simple_2() {
        let common_base = Expr::local(LocalVar::new("base", Type::TypedRef("t0".into())));
        let quant1_i = LocalVar::new("i", Type::Int);
        let quant1_j = LocalVar::new("j", Type::Int);
        let quant2_i = LocalVar::new("i", Type::Int);
        let quant2_j = LocalVar::new("j", Type::Int);
        {
            let quant1 = quant_resource_builder_sample_1(&quant1_i, &quant1_j, &common_base, true);
            let quant2 = quant_resource_builder_sample_1(&quant2_i, &quant2_j, &common_base, false);
            assert!(quant1.is_similar_to(&quant2, false));
        }
        {
            let quant1 = quant_resource_builder_sample_1(&quant1_i, &quant1_j, &common_base, true);
            let quant2 = quant_resource_builder_sample_1(&quant2_i, &quant2_j, &common_base, true);
            assert!(quant1.is_similar_to(&quant2, false));
        }
    }

    #[test]
    fn test_quant_resource_access_similarity_failure_simple() {
        // 1
        {
            let base_1 = Expr::local(LocalVar::new("base", Type::TypedRef("t0".into())));
            let base_2 = Expr::local(LocalVar::new("baaaaase", Type::TypedRef("t0".into())));
            let quant1_i = LocalVar::new("a", Type::Int);
            let quant1_j = LocalVar::new("b", Type::Int);
            let quant2_i = LocalVar::new("i", Type::Int);
            let quant2_j = LocalVar::new("j", Type::Int);
            let quant1 = quant_resource_builder_sample_1(&quant1_i, &quant1_j, &base_1, true);
            let quant2 = quant_resource_builder_sample_1(&quant2_i, &quant2_j, &base_2, false);
            // Differing in the base
            assert!(!quant1.is_similar_to(&quant2, false));
        }
        // 2
        {
            let base_1 = Expr::local(LocalVar::new("base", Type::TypedRef("t0".into())));
            let base_2 = Expr::local(LocalVar::new("base", Type::TypedRef("t0".into())))
                .field(Field {
                    name: "x".to_string(),
                    typ: Type::TypedSeq("tt".into())
                });
            let quant1_i = LocalVar::new("a", Type::Int);
            let quant1_j = LocalVar::new("b", Type::Int);
            let quant2_i = LocalVar::new("i", Type::Int);
            let quant2_j = LocalVar::new("j", Type::Int);
            let quant1 = quant_resource_builder_sample_1(&quant1_i, &quant1_j, &base_1, true);
            let quant2 = quant_resource_builder_sample_1(&quant2_i, &quant2_j, &base_2, false);
            // One quant. has an extra suffix
            assert!(!quant1.is_similar_to(&quant2, false));
        }
    }

    #[test]
    fn test_quant_resource_access_try_instantiate_simple_1() {
        let base = Expr::local(LocalVar::new("base", Type::TypedRef("t0".into())));
        let i = LocalVar::new("i", Type::Int);
        let j = LocalVar::new("j", Type::Int);

        let quant = quant_resource_builder_sample_1(&i, &j, &base, true);
        // foo.bar.value
        let foo = Expr::local(LocalVar::new("foo", Type::TypedRef("foo".into())))
            .field(Field { name: "bar".into(), typ: Type::TypedRef("bar".into()) })
            .field(Field { name: "value".into(), typ: Type::Int });
        // base.a.val_array[42 + 2 * foo.bar.value].val_ref
        let target_place =
            array_access_builder_sample_1(
                &Expr::Const(Int(42), Position::default()), &foo, &base
            );
        let result = quant.try_instantiate(&target_place);
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.is_fully_instantiated());
        assert!(result.is_match_perfect());
        assert_eq!(result.target_place_expr, target_place); // Will always be true, no matter how it is instantiated
        assert_eq!(result.instantiated.resource.get_place(), &target_place);
        assert_eq!(
            &*result.instantiated.cond,
            &Expr::lt_cmp(Expr::Const(Int(42), Position::default()), foo.clone())
        );
    }

    // Like the previous one, except that the target has an extra suffix
    #[test]
    fn test_quant_resource_access_try_instantiate_simple_2() {
        let base = Expr::local(LocalVar::new("base", Type::TypedRef("t0".into())));
        let i = LocalVar::new("i", Type::Int);
        let j = LocalVar::new("j", Type::Int);

        let quant = quant_resource_builder_sample_1(&i, &j, &base, true);
        // foo.bar.value
        let foo = Expr::local(LocalVar::new("foo", Type::TypedRef("foo".into())))
            .field(Field { name: "bar".into(), typ: Type::TypedRef("bar".into()) })
            .field(Field { name: "value".into(), typ: Type::Int });
        // base.a.val_array[42 + 2 * foo.bar.value].val_ref.foo_bar
        let target_place =
            array_access_builder_sample_1(
                &Expr::Const(Int(42), Position::default()), &foo, &base
            ).field(Field { name: "foo_bar".into(), typ: Type::TypedRef("foo_bar".into()) });
        let result = quant.try_instantiate(&target_place);
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.is_fully_instantiated());
        // `target` has an extra `foo_bar`, so the match is not perfect
        assert!(!result.is_match_perfect());
        assert_eq!(result.target_place_expr, target_place); // Will always be true, no matter how it is instantiated
        assert_ne!(result.instantiated.resource.get_place(), &target_place);
        assert!(target_place.has_proper_prefix(result.instantiated.resource.get_place()));
        assert_eq!(result.instantiated.resource.get_place(), target_place.get_parent_ref().unwrap());
        assert_eq!(
            &*result.instantiated.cond,
            &Expr::lt_cmp(Expr::Const(Int(42), Position::default()), foo.clone())
        );
    }

    // i + 2 * j
    fn index_builder_sample_1(i: &Expr, j: &Expr) -> Expr {
        Expr::BinOp(
            BinOpKind::Add,
            box i.clone(),
            box Expr::BinOp(
                BinOpKind::Mul,
                box Expr::Const(Const::Int(2), Position::default()),
                box j.clone(),
                Position::default()
            ),
            Position::default()
        )
    }

    // base.a.val_array[i + 2 * j].val_ref
    fn array_access_builder_sample_1(
        i: &Expr,
        j: &Expr,
        base: &Expr,
    ) -> Expr {
        let a = Field {
            name: "a".to_string(),
            typ: Type::TypedRef("t1".into()),
        };
        let val_array = Field {
            name: "val_array".to_string(),
            typ: Type::TypedSeq("t2".into()),
        };
        let val_ref = Field {
            name: "val_ref".to_string(),
            typ: Type::TypedRef("t3".into()),
        };
        let idx = index_builder_sample_1(&i, &j);
        Expr::seq_index(
            base.clone()
                .field(a)
                .field(val_array),
            idx
        ).field(val_ref)
    }

    // i < j ==> acc(base.a.val_array[i + 2 * j].val_ref)
    fn quant_resource_builder_sample_1(
        i: &LocalVar,
        j: &LocalVar,
        base: &Expr,
        vars_in_order: bool
    ) -> QuantifiedResourceAccess {
        let vars = if vars_in_order {
            vec![i.clone(), j.clone()]
        } else {
            vec![j.clone(), i.clone()]
        };
        let i_expr = Expr::local(i.clone());
        let j_expr = Expr::local(j.clone());
        let arr_access = array_access_builder_sample_1(&i_expr, &j_expr, base);
        QuantifiedResourceAccess {
            vars,
            triggers: vec![Trigger::new(vec![arr_access.clone()])],
            cond: box Expr::lt_cmp(i_expr.clone(), j_expr.clone()),
            resource: PlainResourceAccess::Field(FieldAccessPredicate {
                place: box arr_access,
                perm: PermAmount::Write,
            })
        }
    }
}