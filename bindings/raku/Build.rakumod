use v6.d;

use Vinary::Tree::Interop;

class Build {
    method build(IO() $distribution --> Bool:D) {
        my $root = $distribution.IO.absolute.IO;
        my $library-name = $*VM.platform-library-name(
            'lling_llang_raku_provider'.IO);
        my $output = $root.add('resources/libraries').add($library-name);
        my $script = $root.add('build-provider.raku');
        my $include = native-header-path().parent;
        my $process = run $*EXECUTABLE, $script,
            "--output=$output", "--interop-include=$include", :out, :err;
        my $stdout = $process.out.slurp-rest;
        my $stderr = $process.err.slurp-rest;
        print $stdout if $stdout.chars;
        note $stderr if $stderr.chars;
        die "Lling-Llang native provider build failed with status " ~
            $process.exitcode if $process.exitcode;
        True
    }
}
