use v6.d;
use Bench;
use Lling::Llang;

sub sample-graph() {
    my $builder = WfstBuilder.new(size-hint => 2);
    my $from = $builder.add-state;
    my $to = $builder.add-state;
    $builder.set-start($from).set-final($to);
    $builder.add-arc($from, 'a', 'b', $to);
    $builder.build
}

my $left = sample-graph;
my $right = sample-graph;
my $bench = Bench.new;
$bench.timethese(10_000, {
    compose-capture => {
        my $product = compose($left, $right);
        $product.close;
    },
    state-expansion => { $left.arcs(0) },
});
$left.close;
$right.close;
