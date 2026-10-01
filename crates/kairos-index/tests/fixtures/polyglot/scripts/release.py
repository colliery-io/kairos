"""Bump the version. A repository rule can exclude this folder."""


def bump_version(version):
    major, minor, patch = version.split(".")
    return f"{major}.{minor}.{int(patch) + 1}"
