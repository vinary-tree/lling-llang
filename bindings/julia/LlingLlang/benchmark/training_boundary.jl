using LlingLlang

function frame_loop()
    builder = WfstBuilder{UInt64,LogWeight}(size_hint=1)
    state = add_state!(builder)
    set_start!(builder, state)
    set_final!(builder, state, LogWeight(0.0))
    add_arc!(builder, state, UInt64(0), UInt64(0), state, LogWeight(0.0))
    build!(builder)
end

function measure_ms(operation, repetitions)
    operation()
    elapsed = Float64[]
    for _ in 1:repetitions
        started = time_ns()
        operation()
        push!(elapsed, (time_ns() - started) / 1e6)
    end
    sort!(elapsed)
    elapsed[cld(length(elapsed), 2)]
end

frames = parse(Int, get(ENV, "LLING_BENCH_FRAMES", "256"))
pdfs = parse(Int, get(ENV, "LLING_BENCH_PDFS", "64"))
repetitions = parse(Int, get(ENV, "LLING_BENCH_REPETITIONS", "7"))
1 <= frames && 1 <= pdfs && frames * pdfs <= 1_048_576 ||
    error("benchmark matrix exceeds the native cell limit")
repetitions > 0 || error("repetitions must be positive")

scores = fill(-log(Float64(pdfs)), frames, pdfs)
graph = frame_loop()
config = LfMmiConfig(leaky_hmm_coefficient=0, l2_regularize=0,
    xent_regularize=0)
try
    boundary = () -> lfmmi_loss(scores, graph, graph; config)
    println("matrix=$(frames)x$(pdfs) bytes=$(sizeof(scores)) repetitions=$repetitions")
    println("host LF-MMI median_ms=$(measure_ms(boundary, repetitions))",
        " julia_alloc_bytes=$(@allocated boundary())")
    println("host staging copy median_ms=$(measure_ms(() -> copy(scores), repetitions))")

    # Run in an environment with CUDA installed and set LLING_BENCH_CUDA=1
    # to measure the device staging required by this host-only native ABI.
    if get(ENV, "LLING_BENCH_CUDA", "0") == "1"
        @eval import CUDA
        CUDA.functional() || error("CUDA is not functional in this environment")
        device = CUDA.CuArray(scores)
        println("host to device median_ms=$(measure_ms(() -> begin
            copyto!(device, scores)
            CUDA.synchronize()
        end, repetitions))")
        println("device to host median_ms=$(measure_ms(() -> Array(device), repetitions))")
    end
finally
    close(graph)
end
