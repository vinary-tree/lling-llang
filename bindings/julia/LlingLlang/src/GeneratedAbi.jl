# Generated from bindings/api.json. Do not edit by hand.
const ABI_VERSION = UInt32(1)
const API_REVISION = UInt32(3)

@enum Status::UInt32 begin
    STATUS_OK = 0
    STATUS_INVALID_ARGUMENT = 1
    STATUS_NULL_POINTER = 2
    STATUS_PANIC = 3
    STATUS_INCOMPATIBLE_RESOURCE = 4
    STATUS_PROVIDER_ERROR = 5
    STATUS_LIMIT_EXCEEDED = 6
    STATUS_CLOSED = 7
end
