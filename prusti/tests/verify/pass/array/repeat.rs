extern crate prusti_contracts;

// ignore-test These causes Silicon to genererate ill-formed SMT. See https://github.com/viperproject/silicon/issues/486

const FOO: usize = 42;

// Carbon cannot verify that result[i] == 12 unfortunately, albeit seemingly having all definitions in scope
// #[ensures="forall i: usize :: (0 <= i && i < FOO) ==> result[i] == 12"]
fn return_lit_1() -> [usize; FOO] {
    let a = [12; FOO];
    a
}

// Though it can verifiy this one
#[ensures="forall i: usize :: (0 <= i && i < FOO) ==> result[i] == 24"]
fn return_lit_2() -> [usize; FOO] {
    [24; FOO]
}

// TODO: The postcondition is not correctly translated. It lacks an 'unfolding in' for 'value'
// Generated: 
// (unfolding acc(array$usize$42(_0), write) in (forall i: Int :: 0 <= i && i < 42 ==> 0 <= i && i < |_0.val_array|)) 
// && (unfolding acc(array$usize$42(_0), write) in (let _LET_0 == (old[pre](_1.val_int)) in (forall i: Int :: 0 <= i && i < 42 ==> (unfolding acc(usize(_0.val_array[i].val_ref), write) in i < |_0.val_array| && _0.val_array[i].val_ref.val_int == _LET_0))))
//                                                                 ^^^^^^^^^^^^^^^^^^^^ missing access
// Should be:  
// (unfolding acc(array$usize$42(_0), write) in (forall i: Int :: 0 <= i && i < 42 ==> 0 <= i && i < |_0.val_array|)) 
// && (unfolding acc(array$usize$42(_0), write) in unfolding acc(old[pre](_1.val_int), read$()) in (let _LET_0 == (old[pre](_1.val_int)) in (forall i: Int :: 0 <= i && i < 42 ==> (unfolding acc(usize(_0.val_array[i].val_ref), write) in i < |_0.val_array| && _0.val_array[i].val_ref.val_int == _LET_0))))
//                                                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^               
//
// #[ensures="forall i: usize :: (0 <= i && i < FOO) ==> result[i] == value"]
fn return_val_1(value: usize) -> [usize; FOO] {
    let a = [value; FOO];
    a
}

// Ditto
// #[ensures="forall i: usize :: (0 <= i && i < FOO) ==> result[i] == value"]
fn return_val_2(value: usize) -> [usize; FOO] {
    [value; FOO]
}

#[trusted]
fn main() {}