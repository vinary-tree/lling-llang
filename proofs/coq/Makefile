# lling-llang Coq/Rocq proofs Makefile
#
# The repository-level proofs/verify.sh wrapper supplies the mandatory local
# 4 GiB RSS cap, disables swap, and serializes this build.  Direct CI use must
# provide an equivalent runner-level memory limit.

COQMAKEFILE = CoqMakefile
COQMAKE = $(MAKE) -f $(COQMAKEFILE)

.PHONY: all clean force proof-check

all: $(COQMAKEFILE)
	$(COQMAKE)

$(COQMAKEFILE): _CoqProject
	rocq makefile -f _CoqProject -o $@

clean:
	if [ -f $(COQMAKEFILE) ]; then $(COQMAKE) clean; fi
	rm -f $(COQMAKEFILE) $(COQMAKEFILE).conf
	rm -f *.glob *.vo *.vok *.vos *.aux .*.aux .lia.cache .nia.cache .nra.cache
	rm -f foundations/*.glob foundations/*.vo foundations/*.vok foundations/*.vos foundations/*.aux foundations/.*.aux
	rm -f wfst/*.glob wfst/*.vo wfst/*.vok wfst/*.vos wfst/*.aux wfst/.*.aux
	rm -f algorithms/*.glob algorithms/*.vo algorithms/*.vok algorithms/*.vos algorithms/*.aux algorithms/.*.aux
	rm -f abi/*.glob abi/*.vo abi/*.vok abi/*.vos abi/*.aux abi/.*.aux
	rm -f optimizer/*.glob optimizer/*.vo optimizer/*.vok optimizer/*.vos optimizer/*.aux optimizer/.*.aux
	rm -f domain_integration/*.glob domain_integration/*.vo domain_integration/*.vok domain_integration/*.vos domain_integration/*.aux domain_integration/.*.aux
	rm -f logict/*.glob logict/*.vo logict/*.vok logict/*.vos logict/*.aux logict/.*.aux
	rm -f presburger/*.glob presburger/*.vo presburger/*.vok presburger/*.vos presburger/*.aux presburger/.*.aux
	rm -f sft/*.glob sft/*.vo sft/*.vok sft/*.vos sft/*.aux sft/.*.aux
	rm -f symbolic_algebra/*.glob symbolic_algebra/*.vo symbolic_algebra/*.vok symbolic_algebra/*.vos symbolic_algebra/*.aux symbolic_algebra/.*.aux

force:
	touch _CoqProject
	$(MAKE) all

# Check for Admitted proofs (CI integration)
proof-check:
	@echo "Checking for unchecked proof escapes..."
	@python3 ../../scripts/check-rocq-proof-escapes.py
