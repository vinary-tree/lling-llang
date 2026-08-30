#!/usr/bin/env raku

use v6.d;

sub usage(--> Nil) {
    note 'usage: raku bindings/raku/build-provider.raku --output=PATH --interop-include=DIR';
    exit 2;
}

my %options = @*ARGS.map({ .split('=', 2) }).map({ .[0] => .[1] });
my $output = %options<--output> // usage;
my $include = %options<--interop-include> // usage;
my $source = $?FILE.IO.absolute.IO.parent.add('cbits/provider.c');
my $compiler = %*ENV<CC> // 'cc';

$output.IO.parent.mkdir;
my @command;
if $*KERNEL.name eq 'win32' && $compiler.IO.basename.lc eq any(<cl cl.exe>) {
    @command = $compiler, </nologo /std:c11 /O2 /W4 /WX /LD>,
        "/I$include", $source.Str, '/link', "/OUT:$output";
} else {
    my @flags = $*KERNEL.name eq 'darwin'
        ?? <-std=c17 -O3 -Wall -Wextra -Werror -fPIC -dynamiclib>
        !! <-std=c17 -O3 -Wall -Wextra -Werror -fPIC -shared>;
    @command = $compiler, |@flags, "-I$include", $source.Str, '-o', $output;
}
my $process = run |@command, :out, :err;
my $stdout = $process.out.slurp-rest;
my $stderr = $process.err.slurp-rest;
print $stdout;
note $stderr if $stderr.chars;
exit $process.exitcode;
