# COLLIERY-T-1851: updates and merges of the index.
# An update builds the structure again from the tree, and it runs the
# summarizer only for the keys that the summary pool does not have. A merge
# is an update of the merged tree with the pools of the 2 indexes. The
# scenarios use the fake summarizer and the deterministic vectors.
Feature: Updates and merges

  Scenario: An update summarizes only the changed symbols
    Given a summarized index of the polyglot fixture
    When I change the body of one Rust function and update the index
    Then the summarizer ran for that function and for its file and module only
    And each other summary is the same as before

  Scenario: A formatting change costs nothing
    Given a summarized index of the polyglot fixture
    When I reformat a Python file and update the index
    Then the summarizer did not run

  Scenario: A removed symbol leaves the structure
    Given a summarized index of the polyglot fixture
    When I delete a TypeScript function and update the index
    Then the function and its edges are not in the structure

  Scenario: Two branches merge with no conflict
    Given an index of branch A, where one Go function changed
    And an index of branch B, where one Rust function changed
    When I merge the 2 indexes for the merged tree
    Then the merged index has both changed functions with their new summaries
    And the summarizer did not run

  Scenario: A changed callee signature gives new summaries to its direct callers only
    Given a summarized index, where function A calls B, and B calls C
    When the signature of C changes and I update the index
    Then C and B are summarized again
    And A is not summarized again

  Scenario: A moved file gets a new file summary, and its symbols keep theirs
    Given a summarized index of the polyglot fixture
    When I move a Python file to another folder and update the index
    Then the file is summarized again
    And its symbols are not summarized again

  Scenario: A local update does not run SCIP
    Given a summarized index with Rust edges from SCIP
    When I change a Rust function and update the index with no options
    Then SCIP did not run
    And the edges of the changed file have name classes, marked to be replaced
    When I update the index with "--rust-edges"
    Then SCIP ran, and the marked edges are SCIP edges again

  # rust-analyzer expands format! and each macro_rules! macro that keeps its
  # input as code, also with proc macros off, so SCIP resolves the call in the
  # format! argument. The fallback is for a macro that keeps its input as
  # text, as a proc macro does when proc macros are off (Dylan, 2026-10-01).
  Scenario: A call inside a macro gets a name class
    Given the polyglot fixture, where a Rust function calls a fixture function inside a format! argument and inside a custom macro that keeps its input as text
    When I build the index
    Then the call inside the custom macro is an edge with a name class and the source "macro-text"
    And the call inside the format! argument is a SCIP edge
    And no call that SCIP resolved is also an edge from the macro text

  Scenario: An unchanged symbol in a changed file keeps its SCIP edges
    Given a summarized index with Rust edges from SCIP
    When I change one function in a Rust file and update the index with no options
    Then the other functions of that file keep their SCIP edges and their keys
    And only the changed function has name-class edges, marked to be replaced
    And the summarizer did not run for the unchanged functions

  Scenario: Uncommitted changes are in the index
    Given a summarized index of the polyglot fixture
    When I change a file and do not commit it, and update the index
    Then the index describes the changed file
