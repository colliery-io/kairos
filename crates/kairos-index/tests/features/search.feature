# COLLIERY-T-1857 (Dylan, 2026-10-01): code_search ranks test code below the
# other code. With no summaries, the search reads the names, the signatures
# and the paths, and the names of test functions often have the words of the
# query.
Feature: The search of the code

  Scenario: code_search ranks test code below the other code
    Given an index of the polyglot fixture with no summaries
    When I search the code for "summary counts the rows"
    Then the results have test functions and functions that are not test code
    And each result that is not test code ranks above each test function
