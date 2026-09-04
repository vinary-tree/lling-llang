unit module Lling::Llang::GeneratedAbi;

# Generated from bindings/api.json. Do not edit by hand.
our constant ABI-VERSION is export = 1;
our constant API-REVISION is export = 6;
our constant TYPED-ABI-VERSION is export = 2;
our constant DESCRIPTOR-SIGNATURE-KNOWN is export = 1 +< 0;
our constant DESCRIPTOR-SNAPSHOT-PRESENT is export = 1 +< 1;
our constant DESCRIPTOR-CONTEXT-PRESENT is export = 1 +< 2;
our constant BUDGET-STATES is export = 1 +< 0;
our constant BUDGET-ARCS is export = 1 +< 1;
our constant BUDGET-BYTES is export = 1 +< 2;
our constant BUDGET-WORK is export = 1 +< 3;

our enum Status is export (
    OK => 0,
    INVALID-ARGUMENT => 1,
    NULL-POINTER => 2,
    PANIC => 3,
    INCOMPATIBLE-RESOURCE => 4,
    PROVIDER-ERROR => 5,
    LIMIT-EXCEEDED => 6,
    CLOSED => 7,
);
