extern crate prusti_contracts;

// ignore-test Some tests fail on Silicon, but all succeed on Carbon.

#[derive(Copy, Clone)]
struct Foo {
    value: usize,
    bar: Bar,
}

#[derive(Copy, Clone)]
struct Bar {
    value: usize,
}

#[requires="arr.len() > 1"]
fn return_fixed(arr: &[Foo]) -> Foo {
    arr[1]
}

// We need i >= 0 because unsigned integers bounds are not encoded by default
#[requires="0 <= i && i < arr.len()"]
fn return_nth(arr: &[Foo], i: usize) -> Foo {
    arr[i]
}

#[requires="0 <= i && i < arr.len()"]
fn return_nth_from_ref(arr: &[Foo], i: usize) -> Foo {
    let a = &arr[i];
    *a
}

// TODO: This one causes a crash
/*
#[requires="0 <= i && i < 64"]
#[requires="0 <= j && j < 64"]
#[requires="0 <= k && k < 64"]
fn sum_many(arr: &[Foo], i: usize, j: usize, k: usize) -> usize {
    arr[i].value + arr[j].bar.value + arr[k].value
}
*/

#[requires="0 <= i && i < arr.len()"]
#[requires="0 <= j && j < arr.len()"]
#[requires="0 <= k && k < arr.len()"]
fn sum_many_from_ref(arr: &[Foo], i: usize, j: usize, k: usize) -> usize {
    let a = &arr[i];
    let b = &arr[j];
    let c = &arr[k];
    a.value + b.bar.value + c.value
}

// With &mut

#[requires="arr.len() > 1"]
fn return_fixed_mut(arr: &mut [Foo]) -> Foo {
    arr[1]
}

// We need i >= 0 because unsigned integers bounds are not encoded by default
#[requires="0 <= i && i < arr.len()"]
fn return_nth_mut(arr: &mut [Foo], i: usize) -> Foo {
    arr[i]
}

#[requires="0 <= i && i < arr.len()"]
fn return_nth_from_ref_mut(arr: &mut [Foo], i: usize) -> Foo {
    let a = &arr[i];
    *a
}
// TODO: This one causes a crash
/*
#[requires="0 <= i && i < arr.len()"]
#[requires="0 <= j && j < arr.len()"]
#[requires="0 <= k && k < arr.len()"]
fn sum_many_mut(arr: &mut [Foo], i: usize, j: usize, k: usize) -> Foo {
    arr[i].value + arr[j].bar.value + arr[k].value
}
*/

#[requires="0 <= i && i < arr.len()"]
#[requires="0 <= j && j < arr.len()"]
#[requires="0 <= k && k < arr.len()"]
fn sum_many_from_ref_mut(arr: &mut [Foo], i: usize, j: usize, k: usize) -> usize {
    let a = &arr[i];
    let b = &arr[j];
    let c = &arr[k];
    a.value + b.bar.value + c.value
}

fn main() {}