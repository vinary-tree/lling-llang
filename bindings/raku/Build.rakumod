use v6.d;

use Vinary::Tree::Interop;

class Build {
    method build(IO() $distribution --> Bool:D) {
        my $root = $distribution.IO.absolute.IO;
        my $library-name = $*VM.platform-library-name(
            'lling_llang_raku_provider'.IO);
        my $output = $root.add('resources/libraries').add($library-name);
        my $script = $root.add('build-provider.raku');
        my $include = $root.add('.build/interop-include');
        my $header = materialize-native-header($include);
        LEAVE {
            $header.unlink if $header.e;
            $include.rmdir if $include.d && $include.dir.elems == 0;
            my $build = $include.parent;
            $build.rmdir if $build.d && $build.dir.elems == 0;
        }
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
