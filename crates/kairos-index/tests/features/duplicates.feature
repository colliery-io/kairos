# COLLIERY-T-1857: the index finds repeated code. There are 3 kinds:
#   exact      the same normalized tree (the same code, with other
#              whitespace or comments);
#   near       a copy with renamed names or a few changed lines, from the
#              token vector of each function;
#   same-idea  2 functions that do the same job in different code, from the
#              summary vectors.
# The fixture's expected-duplicates.toml names the groups and the unrelated
# pairs. The same-idea scenario gives the 2 TypeScript functions the same
# summary text, and its vectors read the summary text only (the summarizer
# embeds `name: summary`), because the deterministic vectors have no meaning.
Feature: Repeated code

  Scenario: An exact copy is found
    Given the polyglot fixture, where a Rust function is copied to a second file with different comments and whitespace
    When I ask for duplicates
    Then the 2 functions are a group of kind "exact"

  Scenario: A copy with renamed names is found
    Given the polyglot fixture, where a Python function is copied with its variables renamed and one line changed
    When I ask for duplicates
    Then the 2 functions are a group of kind "near"

  Scenario: The same idea in different code is found
    Given the polyglot fixture, where 2 TypeScript functions do the same job with different code
    And the fake summarizer gives them summaries with the same meaning
    When I ask for duplicates
    Then the 2 functions are a group of kind "same-idea"

  Scenario: Unrelated code is not a duplicate
    Given the polyglot fixture
    When I ask for duplicates
    Then no group has 2 symbols that the fixture's expected file marks as unrelated
    And the groups are the groups of the fixture's expected file

  Scenario: Small symbols and test code are left out by default
    Given the polyglot fixture, where 2 one-line getters are the same, and 2 test functions are the same
    When I ask for duplicates
    Then neither pair is in a group
    When I ask for duplicates with no size limit and with test code
    Then both pairs are groups

  Scenario: An update recomputes only the changed token vectors
    Given an index with token vectors
    When I change one function and update the index
    Then only that function's token vector is made again
