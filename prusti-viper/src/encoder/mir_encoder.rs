// © 2019, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use encoder::builtin_encoder::BuiltinFunctionKind;
use encoder::error_manager::ErrorCtxt;
use encoder::vir;
use encoder::Encoder;
use prusti_interface::config;
use rustc::hir::def_id::DefId;
use rustc::mir;
use rustc::ty;
use rustc_data_structures::indexed_vec::Idx;
use std;
use syntax::ast;
use syntax::codemap::Span;
use encoder::type_encoder::TypeEncoder;

pub static PRECONDITION_LABEL: &'static str = "pre";
pub static POSTCONDITION_LABEL: &'static str = "post";
pub static WAND_LHS_LABEL: &'static str = "lhs";

/// Common code used for `ProcedureEncoder` and `PureFunctionEncoder`
#[derive(Clone)]
pub struct MirEncoder<'p, 'v: 'p, 'r: 'v, 'a: 'r, 'tcx: 'a> {
    encoder: &'p Encoder<'v, 'r, 'a, 'tcx>,
    mir: &'p mir::Mir<'tcx>,
    def_id: DefId,
    namespace: String,
}

impl<'p, 'v: 'p, 'r: 'v, 'a: 'r, 'tcx: 'a> MirEncoder<'p, 'v, 'r, 'a, 'tcx> {
    pub fn new(
        encoder: &'p Encoder<'v, 'r, 'a, 'tcx>,
        mir: &'p mir::Mir<'tcx>,
        def_id: DefId,
    ) -> Self {
        trace!("MirEncoder constructor");
        MirEncoder {
            encoder,
            mir,
            def_id,
            namespace: "".to_string(),
        }
    }

    pub fn new_with_namespace(
        encoder: &'p Encoder<'v, 'r, 'a, 'tcx>,
        mir: &'p mir::Mir<'tcx>,
        def_id: DefId,
        namespace: String,
    ) -> Self {
        trace!("MirEncoder constructor with namespace");
        MirEncoder {
            encoder,
            mir,
            def_id,
            namespace,
        }
    }

    pub fn encode_local_var_name(&self, local: mir::Local) -> String {
        format!("{}{:?}", self.namespace, local)
    }

    pub fn get_local_ty(&self, local: mir::Local) -> ty::Ty<'tcx> {
        self.mir.local_decls[local].ty
    }

    pub fn encode_local(&self, local: mir::Local) -> vir::LocalVar {
        let var_name = self.encode_local_var_name(local);
        let local_ty = self.get_local_ty(local);
        match local_ty.sty {
            ty::TypeVariants::TyArray(inner, _)
            | ty::TypeVariants::TySlice(inner) => {
                let type_name = self
                    .encoder
                    .encode_type_predicate_use(inner);
                vir::LocalVar::new(var_name, vir::Type::TypedSeq(type_name))
            }
            _ => {
                let type_name = self
                    .encoder
                    .encode_type_predicate_use(local_ty);
                vir::LocalVar::new(var_name, vir::Type::TypedRef(type_name))
            }
        }
    }

    /// Returns
    /// - `vir::Expr`: the expression of the projection;
    /// - `ty::Ty<'tcx>`: the type of the expression;
    /// - `Option<usize>`: optionally, the variant of the enum.
    pub fn encode_place(
        &self,
        place: &mir::Place<'tcx>,
    ) -> (vir::Expr, ty::Ty<'tcx>, Option<usize>) {
        trace!("Encode place {:?}", place);
        match place {
            &mir::Place::Local(local) => (
                self.encode_local(local).into(),
                self.get_local_ty(local),
                None,
            ),

            &mir::Place::Projection(ref place_projection) => {
                self.encode_projection(place_projection)
            }

            x => unimplemented!("{:?}", x),
        }
    }

    /// Returns
    /// - `vir::Expr`: the place of the projection;
    /// - `ty::Ty<'tcx>`: the type of the place;
    /// - `Option<usize>`: optionally, the variant of the enum.
    fn encode_projection(
        &self,
        place_projection: &mir::PlaceProjection<'tcx>,
    ) -> (vir::Expr, ty::Ty<'tcx>, Option<usize>) {
        trace!("Encode projection {:?}", place_projection);
        let (encoded_base, base_ty, opt_variant_index) = self.encode_place(&place_projection.base);

        trace!("place_projection: {:?}", place_projection);
        trace!("encoded_base: {:?}", encoded_base);
        trace!("base_ty: {:?}", base_ty);

        match &place_projection.elem {
            &mir::ProjectionElem::Field(ref field, _) => {
                match base_ty.sty {
                    ty::TypeVariants::TyBool
                    | ty::TypeVariants::TyInt(_)
                    | ty::TypeVariants::TyUint(_)
                    | ty::TypeVariants::TyRawPtr(_)
                    | ty::TypeVariants::TyRef(_, _, _) => {
                        panic!("Type {:?} has no fields", base_ty)
                    }

                    ty::TypeVariants::TyTuple(elems) => {
                        let field_name = format!("tuple_{}", field.index());
                        let field_ty = elems[field.index()];
                        let encoded_field = self.encoder.encode_raw_ref_field(field_name, field_ty);
                        let encoded_projection = encoded_base.field(encoded_field);
                        (encoded_projection, field_ty, None)
                    }

                    ty::TypeVariants::TyAdt(ref adt_def, ref subst) if !adt_def.is_box() => {
                        debug!("subst {:?}", subst);
                        let num_variants = adt_def.variants.len();
                        // FIXME: why this can be None?
                        let variant_index = opt_variant_index.unwrap_or_else(|| {
                            assert_eq!(num_variants, 1);
                            0
                        });
                        let tcx = self.encoder.env().tcx();
                        let variant_def = &adt_def.variants[variant_index];
                        let encoded_variant = if num_variants != 1 {
                            encoded_base.variant(&variant_def.name.as_str())
                        } else {
                            encoded_base
                        };
                        let field = &variant_def.fields[field.index()];
                        let field_ty = field.ty(tcx, subst);
                        let encoded_field = self
                            .encoder
                            .encode_struct_field(&field.ident.as_str(), field_ty);
                        let encoded_projection = encoded_variant.field(encoded_field);
                        (encoded_projection, field_ty, None)
                    }

                    ty::TypeVariants::TyClosure(def_id, ref closure_subst) => {
                        debug!("closure_subst {:?}", closure_subst);
                        let tcx = self.encoder.env().tcx();
                        let node_id = tcx.hir.as_local_node_id(def_id).unwrap();
                        let field_ty = closure_subst
                            .upvar_tys(def_id, tcx)
                            .nth(field.index())
                            .unwrap();

                        let encoded_projection: vir::Expr = tcx.with_freevars(node_id, |freevars| {
                            let freevar = &freevars[field.index()];
                            let field_name = format!("closure_{}", field.index());
                            let encoded_field = self.encoder.encode_raw_ref_field(field_name, field_ty);
                            let res = encoded_base.field(encoded_field);
                            let var_name = tcx.hir.name(freevar.var_id()).to_string();
                            trace!("Field {:?} of closure corresponds to variable '{}', encoded as {}", field, var_name, res);
                            res
                        });

                        let encoded_field_type = self.encoder.encode_type(field_ty);
                        debug!("Rust closure projection {:?}", place_projection);
                        debug!("encoded_projection: {:?}", encoded_projection);

                        assert_eq!(encoded_projection.get_type(), encoded_field_type);

                        (encoded_projection, field_ty, None)
                    }

                    ref x => unimplemented!("{:?}", x),
                }
            }

            &mir::ProjectionElem::Deref => self.encode_deref(encoded_base, base_ty),

            &mir::ProjectionElem::Downcast(ref adt_def, variant_index) => {
                debug!("Downcast projection {:?}, {:?}", adt_def, variant_index);
                (encoded_base, base_ty, Some(variant_index))
            }

            &mir::ProjectionElem::Index(index) => {
                let projection_ty = match base_ty.sty {
                    ty::TypeVariants::TyArray(ty, _)
                    | ty::TypeVariants::TySlice(ty) => ty,
                    _ => unreachable!()
                };
                let encoded_index = vir::Expr::local(self.encode_local(index));
                let val_array_field = TypeEncoder::new(self.encoder, base_ty)
                    .encode_value_field();
                let val_int_field = TypeEncoder::new(self.encoder, self.get_local_ty(index))
                    .encode_value_field();
                let encoded_projection = vir::Expr::seq_index(
                    encoded_base.field(val_array_field),
                    encoded_index.field(val_int_field),
                );
                (encoded_projection, projection_ty, None)
            }

            x => unimplemented!("{:?}", x),
        }
    }

    pub fn is_reference(&self, base_ty: ty::Ty<'tcx>) -> bool {
        trace!("is_reference {}", base_ty);
        match base_ty.sty {
            ty::TypeVariants::TyRawPtr(..) | ty::TypeVariants::TyRef(..) => true,

            _ => false,
        }
    }

    pub fn can_be_dereferenced(&self, base_ty: ty::Ty<'tcx>) -> bool {
        trace!("can_be_dereferenced {}", base_ty);
        match base_ty.sty {
            ty::TypeVariants::TyRawPtr(..) | ty::TypeVariants::TyRef(..) => true,

            ty::TypeVariants::TyAdt(ref adt_def, ..) if adt_def.is_box() => true,

            _ => false,
        }
    }

    pub fn encode_deref(
        &self,
        encoded_base: vir::Expr,
        base_ty: ty::Ty<'tcx>,
    ) -> (vir::Expr, ty::Ty<'tcx>, Option<usize>) {
        trace!("encode_deref {} {}", encoded_base, base_ty);
        assert!(
            self.can_be_dereferenced(base_ty),
            "Type {:?} can not be dereferenced",
            base_ty
        );
        match base_ty.sty {
            ty::TypeVariants::TyRawPtr(ty::TypeAndMut { ty, .. })
            | ty::TypeVariants::TyRef(_, ty, _) => {
                let access = if encoded_base.is_addr_of() {
                    encoded_base.get_parent().unwrap()
                } else {
                    match encoded_base {
                        vir::Expr::AddrOf(box base_base_place, _, _) => base_base_place,
                        _ => {
                            let ref_field = self.encoder.encode_dereference_field(ty);
                            encoded_base.field(ref_field)
                        }
                    }
                };
                (access, ty, None)
            }
            ty::TypeVariants::TyAdt(ref adt_def, ref _subst) if adt_def.is_box() => {
                let access = if encoded_base.is_addr_of() {
                    encoded_base.get_parent().unwrap()
                } else {
                    let field_ty = base_ty.boxed_ty();
                    let ref_field = self.encoder.encode_dereference_field(field_ty);
                    encoded_base.field(ref_field)
                };
                (access, base_ty.boxed_ty(), None)
            }
            ref x => unimplemented!("{:?}", x),
        }
    }

    pub fn eval_place(&self, place: &mir::Place<'tcx>) -> vir::Expr {
        let (encoded_place, place_ty, _) = self.encode_place(place);
        let value_field = self.encoder.encode_value_field(place_ty);
        encoded_place.field(value_field)
    }

    /// Returns an `vir::Expr` that corresponds to the value of the operand
    pub fn encode_operand_expr(&self, operand: &mir::Operand<'tcx>) -> vir::Expr {
        trace!("Encode operand expr {:?}", operand);
        match operand {
            &mir::Operand::Constant(box mir::Constant {
                literal: mir::Literal::Value { value },
                ..
            }) => self.encoder.encode_const_expr(value),
            &mir::Operand::Copy(ref place) | &mir::Operand::Move(ref place) => {
                let val_place = self.eval_place(&place);
                val_place.into()
            }
            &mir::Operand::Constant(box mir::Constant {
                ty,
                literal: mir::Literal::Promoted { .. },
                ..
            }) => {
                debug!("Incomplete encoding of promoted literal {:?}", operand);

                // Generate a function call that leaves the expression undefined.
                let encoded_type = self.encoder.encode_value_type(ty);
                let function_name =
                    self.encoder
                        .encode_builtin_function_use(BuiltinFunctionKind::Unreachable(
                            encoded_type.clone(),
                        ));
                let pos = self.encoder.error_manager().register(
                    // TODO: use a proper span
                    self.mir.span,
                    ErrorCtxt::PureFunctionCall,
                );
                vir::Expr::func_app(function_name, vec![], vec![], encoded_type, pos)
            }
        }
    }

    pub fn get_operand_ty(&self, operand: &mir::Operand<'tcx>) -> ty::Ty<'tcx> {
        debug!("Get operand ty {:?}", operand);
        match operand {
            &mir::Operand::Move(ref place) | &mir::Operand::Copy(ref place) => {
                let (_, ty, _) = self.encode_place(place);
                ty
            }
            &mir::Operand::Constant(box mir::Constant { ty, .. }) => ty,
        }
    }

    /// Returns an `vir::Type` that corresponds to the type of the value of the operand
    pub fn encode_operand_expr_type(&self, operand: &mir::Operand<'tcx>) -> vir::Type {
        trace!("Encode operand expr {:?}", operand);
        match operand {
            &mir::Operand::Constant(box mir::Constant { ty, .. }) => {
                let ty = self.encoder.resolve_typaram(ty);
                self.encoder.encode_value_type(ty)
            }
            &mir::Operand::Copy(ref place) | &mir::Operand::Move(ref place) => {
                let (encoded_place, place_ty, _) = self.encode_place(place);
                let place_ty = self.encoder.resolve_typaram(place_ty);
                let value_field = self.encoder.encode_value_field(place_ty);
                let val_place = encoded_place.field(value_field);
                val_place.get_type().clone()
            }
        }
    }

    pub fn encode_bin_op_expr(
        &self,
        op: mir::BinOp,
        left: vir::Expr,
        right: vir::Expr,
        ty: ty::Ty<'tcx>,
    ) -> vir::Expr {
        let is_bool = ty.sty == ty::TypeVariants::TyBool;
        match op {
            mir::BinOp::Eq => vir::Expr::eq_cmp(left, right),
            mir::BinOp::Ne => vir::Expr::ne_cmp(left, right),
            mir::BinOp::Gt => vir::Expr::gt_cmp(left, right),
            mir::BinOp::Ge => vir::Expr::ge_cmp(left, right),
            mir::BinOp::Lt => vir::Expr::lt_cmp(left, right),
            mir::BinOp::Le => vir::Expr::le_cmp(left, right),
            mir::BinOp::Add => vir::Expr::add(left, right),
            mir::BinOp::Sub => vir::Expr::sub(left, right),
            mir::BinOp::Rem => vir::Expr::rem(left, right),
            mir::BinOp::Div => vir::Expr::div(left, right),
            mir::BinOp::Mul => vir::Expr::mul(left, right),
            mir::BinOp::BitAnd if is_bool => vir::Expr::and(left, right),
            mir::BinOp::BitOr if is_bool => vir::Expr::or(left, right),
            mir::BinOp::BitXor if is_bool => vir::Expr::xor(left, right),
            x => unimplemented!("{:?}", x),
        }
    }

    pub fn encode_unary_op_expr(&self, op: mir::UnOp, expr: vir::Expr) -> vir::Expr {
        match op {
            mir::UnOp::Not => vir::Expr::not(expr),
            mir::UnOp::Neg => vir::Expr::minus(expr),
        }
    }

    /// Returns `true` is an overflow happened
    pub fn encode_bin_op_check(
        &self,
        op: mir::BinOp,
        left: vir::Expr,
        right: vir::Expr,
        ty: ty::Ty<'tcx>,
    ) -> vir::Expr {
        if !op.is_checkable() || !config::check_binary_operations() {
            false.into()
        } else {
            let result = self.encode_bin_op_expr(op, left.clone(), right.clone(), ty);

            match op {
                mir::BinOp::Add | mir::BinOp::Mul | mir::BinOp::Sub => match ty.sty {
                    // Unsigned
                    ty::TypeVariants::TyUint(ast::UintTy::U8) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::u8::MIN.into()),
                        vir::Expr::gt_cmp(result, std::u8::MAX.into()),
                    ),
                    ty::TypeVariants::TyUint(ast::UintTy::U16) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::u16::MIN.into()),
                        vir::Expr::gt_cmp(result, std::u16::MAX.into()),
                    ),
                    ty::TypeVariants::TyUint(ast::UintTy::U32) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::u32::MIN.into()),
                        vir::Expr::gt_cmp(result, std::u32::MAX.into()),
                    ),
                    ty::TypeVariants::TyUint(ast::UintTy::U64) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::u64::MIN.into()),
                        vir::Expr::gt_cmp(result, std::u64::MAX.into()),
                    ),
                    ty::TypeVariants::TyUint(ast::UintTy::U128) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::u128::MIN.into()),
                        vir::Expr::gt_cmp(result, std::u128::MAX.into()),
                    ),
                    ty::TypeVariants::TyUint(ast::UintTy::Usize) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::usize::MIN.into()),
                        vir::Expr::gt_cmp(result, std::usize::MAX.into()),
                    ),
                    // Signed
                    ty::TypeVariants::TyInt(ast::IntTy::I8) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::i8::MIN.into()),
                        vir::Expr::gt_cmp(result, std::i8::MAX.into()),
                    ),
                    ty::TypeVariants::TyInt(ast::IntTy::I16) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::i16::MIN.into()),
                        vir::Expr::gt_cmp(result, std::i16::MIN.into()),
                    ),
                    ty::TypeVariants::TyInt(ast::IntTy::I32) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::i32::MIN.into()),
                        vir::Expr::gt_cmp(result, std::i32::MAX.into()),
                    ),
                    ty::TypeVariants::TyInt(ast::IntTy::I64) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::i64::MIN.into()),
                        vir::Expr::gt_cmp(result, std::i64::MAX.into()),
                    ),
                    ty::TypeVariants::TyInt(ast::IntTy::I128) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::i128::MIN.into()),
                        vir::Expr::gt_cmp(result, std::i128::MAX.into()),
                    ),
                    ty::TypeVariants::TyInt(ast::IntTy::Isize) => vir::Expr::or(
                        vir::Expr::lt_cmp(result.clone(), std::isize::MIN.into()),
                        vir::Expr::gt_cmp(result, std::isize::MAX.into()),
                    ),

                    _ => {
                        debug!(
                            "Encoding of bin op check '{:?}' is incomplete for type {:?}",
                            op, ty
                        );
                        false.into()
                    }
                },

                mir::BinOp::Shl | mir::BinOp::Shr => {
                    debug!("Encoding of bin op check '{:?}' is incomplete", op);
                    false.into()
                }

                _ => unreachable!("{:?}", op),
            }
        }
    }

    pub fn encode_cast_expr(
        &self,
        operand: &mir::Operand<'tcx>,
        dst_ty: ty::Ty<'tcx>,
    ) -> vir::Expr {
        let src_ty = self.get_operand_ty(operand);

        let encoded_val = match (&src_ty.sty, &dst_ty.sty) {
            (ty::TypeVariants::TyInt(ast::IntTy::I8), ty::TypeVariants::TyInt(ast::IntTy::I8))
            | (ty::TypeVariants::TyInt(ast::IntTy::I8), ty::TypeVariants::TyInt(ast::IntTy::I16))
            | (ty::TypeVariants::TyInt(ast::IntTy::I8), ty::TypeVariants::TyInt(ast::IntTy::I32))
            | (ty::TypeVariants::TyInt(ast::IntTy::I8), ty::TypeVariants::TyInt(ast::IntTy::I64))
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I8),
                ty::TypeVariants::TyInt(ast::IntTy::I128),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I16),
                ty::TypeVariants::TyInt(ast::IntTy::I16),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I16),
                ty::TypeVariants::TyInt(ast::IntTy::I32),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I16),
                ty::TypeVariants::TyInt(ast::IntTy::I64),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I16),
                ty::TypeVariants::TyInt(ast::IntTy::I128),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I32),
                ty::TypeVariants::TyInt(ast::IntTy::I32),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I32),
                ty::TypeVariants::TyInt(ast::IntTy::I64),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I32),
                ty::TypeVariants::TyInt(ast::IntTy::I128),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I64),
                ty::TypeVariants::TyInt(ast::IntTy::I64),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I64),
                ty::TypeVariants::TyInt(ast::IntTy::I128),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::I128),
                ty::TypeVariants::TyInt(ast::IntTy::I128),
            )
            | (
                ty::TypeVariants::TyInt(ast::IntTy::Isize),
                ty::TypeVariants::TyInt(ast::IntTy::Isize),
            )
            | (ty::TypeVariants::TyChar, ty::TypeVariants::TyChar)
            | (ty::TypeVariants::TyChar, ty::TypeVariants::TyUint(ast::UintTy::U8))
            | (ty::TypeVariants::TyChar, ty::TypeVariants::TyUint(ast::UintTy::U16))
            | (ty::TypeVariants::TyChar, ty::TypeVariants::TyUint(ast::UintTy::U32))
            | (ty::TypeVariants::TyChar, ty::TypeVariants::TyUint(ast::UintTy::U64))
            | (ty::TypeVariants::TyChar, ty::TypeVariants::TyUint(ast::UintTy::U128))
            | (ty::TypeVariants::TyUint(ast::UintTy::U8), ty::TypeVariants::TyChar)
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U8),
                ty::TypeVariants::TyUint(ast::UintTy::U8),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U8),
                ty::TypeVariants::TyUint(ast::UintTy::U16),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U8),
                ty::TypeVariants::TyUint(ast::UintTy::U32),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U8),
                ty::TypeVariants::TyUint(ast::UintTy::U64),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U8),
                ty::TypeVariants::TyUint(ast::UintTy::U128),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U16),
                ty::TypeVariants::TyUint(ast::UintTy::U16),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U16),
                ty::TypeVariants::TyUint(ast::UintTy::U32),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U16),
                ty::TypeVariants::TyUint(ast::UintTy::U64),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U16),
                ty::TypeVariants::TyUint(ast::UintTy::U128),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U32),
                ty::TypeVariants::TyUint(ast::UintTy::U32),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U32),
                ty::TypeVariants::TyUint(ast::UintTy::U64),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U32),
                ty::TypeVariants::TyUint(ast::UintTy::U128),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U64),
                ty::TypeVariants::TyUint(ast::UintTy::U64),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U64),
                ty::TypeVariants::TyUint(ast::UintTy::U128),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::U128),
                ty::TypeVariants::TyUint(ast::UintTy::U128),
            )
            | (
                ty::TypeVariants::TyUint(ast::UintTy::Usize),
                ty::TypeVariants::TyUint(ast::UintTy::Usize),
            ) => self.encode_operand_expr(operand),

            _ => unimplemented!(
                "unimplemented cast from type '{:?}' to type '{:?}'",
                src_ty,
                dst_ty
            ),
        };

        encoded_val
    }

    pub fn encode_operand_place(&self, operand: &mir::Operand<'tcx>) -> Option<vir::Expr> {
        debug!("Encode operand place {:?}", operand);
        match operand {
            &mir::Operand::Move(ref place) | &mir::Operand::Copy(ref place) => {
                let (src, _, _) = self.encode_place(place);
                Some(src)
            }

            &mir::Operand::Constant(_) => None,
        }
    }

    pub fn encode_place_predicate_permission(
        &self,
        place: vir::Expr,
        perm: vir::PermAmount,
    ) -> Option<vir::Expr> {
        vir::Expr::pred_permission(place, perm)
    }

    pub fn encode_old_expr(&self, expr: vir::Expr, label: &str) -> vir::Expr {
        debug!("encode_old_expr {}, {}", expr, label);
        vir::Expr::labelled_old(label, expr)
    }

    pub fn get_span_of_basic_block(&self, bbi: mir::BasicBlock) -> Span {
        let bb_data = &self.mir.basic_blocks()[bbi];
        if bb_data.statements.is_empty() {
            bb_data.terminator.as_ref().unwrap().source_info.span
        } else {
            bb_data.statements[bb_data.statements.len() - 1]
                .source_info
                .span
        }
    }

    pub fn encode_expr_pos(&self, span: Span) -> vir::Position {
        self.encoder
            .error_manager()
            .register(span, ErrorCtxt::GenericExpression)
    }
}
