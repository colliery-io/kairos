# COLLIERY-T-1850: the summaries and the vectors of the index.
# The scenarios use a fake summarizer that gives a fixed text for each input,
# and the deterministic vectors of kairos-embed. Only the @model scenario
# uses the real model. It runs only when the model file is on disk (see
# KAIROS_INDEX_MODEL in tests/bdd.rs) and the test is built with the `llama`
# feature. Otherwise cucumber shows it as skipped, with the reason.
Feature: Summaries

  Scenario: Each symbol that is not a test gets one summary
    Given the polyglot fixture and the fake summarizer
    When I summarize the index
    Then each symbol that is not test code has a summary and a vector
    And no test symbol has a summary

  Scenario: The same code gets the same key
    Given 2 copies of a Rust function that differ only in whitespace and comments
    When I summarize the index
    Then the 2 symbols have the same key
    And the summarizer ran one time for them

  Scenario: A file summary comes from its symbol summaries
    Given the polyglot fixture and the fake summarizer
    When I summarize the index
    Then the input to each file summary is the summaries of its symbols, not the code of the file

  Scenario: A change in a callee does not change the key of its caller
    Given a summarized index
    When the body of a function changes and its signature stays the same
    Then the key of the function changes
    And the keys of its callers do not change

  @model @allow.skipped
  Scenario: The real model writes a summary
    Given the Qwen3-4B model file is on disk
    When I summarize 3 symbols of the polyglot fixture with the real model
    Then each summary has 1 to 5 sentences
    And no summary is empty
