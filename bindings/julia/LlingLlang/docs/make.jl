using Documenter
using LlingLlang

const DOCS_ROOT = @__DIR__
const DOCS_BUILD = get(ENV, "LLING_LLANG_DOCS_BUILD", "build")
const DOCS_DEPLOY = get(ENV, "LLING_LLANG_DOCS_DEPLOY", "0") == "1"

# A deployment must copy the same docs-root build target that makedocs wrote.
DOCS_DEPLOY && DOCS_BUILD != "build" &&
    error("Julia docs deployment requires the docs-root build target")

makedocs(
    root=DOCS_ROOT,
    sitename="LlingLlang.jl",
    modules=[LlingLlang],
    format=Documenter.HTML(
        prettyurls=false,
        repolink="https://github.com/vinary-tree/lling-llang",
    ),
    pages=["Guide and API" => "index.md"],
    build=DOCS_BUILD,
    checkdocs=:exports,
    repo="https://github.com/vinary-tree/lling-llang/blob/{commit}{path}#{line}",
    warnonly=false,
)

isfile(joinpath(DOCS_ROOT, DOCS_BUILD, "index.html")) ||
    error("Documenter did not generate the Julia guide")

if DOCS_DEPLOY
    deploydocs(
        root=DOCS_ROOT,
        target="build",
        repo="github.com/vinary-tree/lling-llang.git",
        devbranch="master",
        push_preview=false,
    )
end
