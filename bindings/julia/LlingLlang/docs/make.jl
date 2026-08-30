using Documenter
using LlingLlang

makedocs(
    sitename="LlingLlang.jl",
    modules=[LlingLlang],
    format=Documenter.HTML(
        prettyurls=false,
        repolink="https://github.com/vinary-tree/lling-llang",
    ),
    pages=["Guide and API" => "index.md"],
    build=get(ENV, "LLING_LLANG_DOCS_BUILD", "build"),
    checkdocs=:exports,
    repo="https://github.com/vinary-tree/lling-llang/blob/{commit}{path}#{line}",
    warnonly=false,
)

if get(ENV, "LLING_LLANG_DOCS_DEPLOY", "0") == "1"
    deploydocs(repo="github.com/vinary-tree/lling-llang.git", devbranch="master")
end
