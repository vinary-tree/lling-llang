unit module Lling::Llang::GeneratedAbi;

# Generated from bindings/api.json. Do not edit by hand.
our constant ABI-VERSION is export = 1;
our constant API-REVISION is export = 4;

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
