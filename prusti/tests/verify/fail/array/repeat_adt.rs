extern crate prusti_contracts;

// ignore-test These causes Silicon to genererate ill-formed SMT. See https://github.com/viperproject/silicon/issues/486

const LEN: usize = 42;

#[derive(Copy, Clone)]
struct Foo {
    value: usize,
    bar: Bar,
}

#[derive(Copy, Clone)]
struct Bar {
    value: usize,
}

#[ensures="forall i: usize :: (0 <= i && i < LEN) ==> (result[i].value == 65 && result[i].bar.value == 1234)"] //~ ERROR postcondition might not hold.
fn return_lit_1() -> [Foo; LEN] {
    let foo = Foo { value: 42, bar: Bar { value: 1234 } };
    let a = [foo; LEN];
    a
}
// TODO: inlining the definition of Foo causes a crash
/*
fn return_lit_2() -> [Foo; LEN] {
    let a = [Foo { value: 42, bar: Bar { value: 1234 } }; LEN];
    a
}
*/

#[ensures="forall i: usize :: (0 <= i && i < LEN) ==> (result[i].value == 42 && result[i].bar.value == 4321)"] //~ ERROR postcondition might not hold.
fn return_lit_3() -> [Foo; LEN] {
    let foo = Foo { value: 42, bar: Bar { value: 1234 } };
    [foo; LEN]
}

// TODO: The postcondition is not correctly translated. It lacks an 'unfolding in' for 'foo.value.val_int'
// See 'pass/repeat_adt' for more details
// #[ensures="forall i: usize :: (0 <= i && i < LEN) ==> (result[i].value == foo.value && result[i].bar.value == foo.bar.value)"]
fn return_val_1(foo: Foo) -> [Foo; LEN] {
    let a = [foo; LEN];
    a
}

// Ditto
// #[ensures="forall i: usize :: (0 <= i && i < LEN) ==> (result[i].value == foo.value && result[i].bar.value == foo.bar.value)"]
fn return_val_2(value: usize) -> [usize; LEN] {
    [value; LEN]
}

#[trusted]
fn main() {}