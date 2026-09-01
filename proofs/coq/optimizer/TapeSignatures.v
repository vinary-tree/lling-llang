(** * TapeSignatures — typed input/output compatibility for optimizer plans

    A transducer has two independently named tape domains.  Composition is
    legal only when the left output domain equals the right input domain.
    Keeping the two projections distinct prevents an optimizer from accepting
    two components merely because their input alphabets happen to agree.
*)

From Stdlib Require Import Arith.Arith.

Record tape_signature : Type := {
  input_domain : nat;
  output_domain : nat
}.

Definition compatible (left right : tape_signature) : Prop :=
  output_domain left = input_domain right.

Definition compose_signature
    (left right : tape_signature) : tape_signature :=
  {| input_domain := input_domain left;
     output_domain := output_domain right |}.

Definition identity_signature (domain : nat) : tape_signature :=
  {| input_domain := domain; output_domain := domain |}.

Theorem compose_signature_associative :
  forall first second third,
    compose_signature (compose_signature first second) third =
    compose_signature first (compose_signature second third).
Proof. intros; destruct first, second, third; reflexivity. Qed.

Theorem left_identity_signature :
  forall signature,
    compatible (identity_signature (input_domain signature)) signature /\
    compose_signature (identity_signature (input_domain signature)) signature =
      signature.
Proof. intros [input output]; simpl; split; reflexivity. Qed.

Theorem right_identity_signature :
  forall signature,
    compatible signature (identity_signature (output_domain signature)) /\
    compose_signature signature (identity_signature (output_domain signature)) =
      signature.
Proof. intros [input output]; simpl; split; reflexivity. Qed.

Theorem compatibility_is_preserved_by_association :
  forall first second third,
    compatible first second ->
    compatible second third ->
    compatible (compose_signature first second) third /\
    compatible first (compose_signature second third).
Proof. intros; split; assumption. Qed.

(** Equal input domains do not establish composability.  This constructive
    counterexample is the regression oracle for implementations that erase the
    output-tape type. *)
Theorem equal_inputs_do_not_imply_compatibility :
  exists left right,
    input_domain left = input_domain right /\
    ~ compatible left right.
Proof.
  exists {| input_domain := 0; output_domain := 1 |}.
  exists {| input_domain := 0; output_domain := 2 |}.
  simpl; split; [reflexivity | discriminate].
Qed.

(** A typed morphism carries its input and output domains in its type. *)
Record morphism (input output : nat) : Type := {
  morphism_token : nat
}.

Definition compose_morphism
    {input middle output : nat}
    (left : morphism input middle)
    (right : morphism middle output) : morphism input output :=
  {| morphism_token := morphism_token _ _ left + morphism_token _ _ right |}.

Definition identity_morphism (domain : nat) : morphism domain domain :=
  {| morphism_token := 0 |}.

Theorem compose_morphism_associative :
  forall input first_middle second_middle output
         (first : morphism input first_middle)
         (second : morphism first_middle second_middle)
         (third : morphism second_middle output),
    morphism_token _ _
      (compose_morphism (compose_morphism first second) third) =
    morphism_token _ _
      (compose_morphism first (compose_morphism second third)).
Proof. intros; simpl; symmetry; apply Nat.add_assoc. Qed.
