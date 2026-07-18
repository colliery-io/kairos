"""
Service management tasks for Kairos.
"""

import angreal  # type: ignore

from utils import docker_up, docker_down, docker_clean

services = angreal.command_group(name="services", about="commands for managing backing services")


@services()
@angreal.command(name="up", about="start backing services for local development")
def up():
    """Start backing services for local development."""
    return docker_up()


@services()
@angreal.command(name="down", about="stop backing services")
@angreal.argument(
    name="volumes",
    long="volumes",
    short="v",
    help="also remove persistent data volumes",
    takes_value=False,
    is_flag=True
)
def down(volumes=False):
    """Stop backing services."""
    return docker_down(volumes)


@services()
@angreal.command(name="reset", about="reset local services (stop and restart)")
@angreal.argument(
    name="clean",
    long="clean",
    short="c",
    help="also clean persistent data volumes",
    takes_value=False,
    is_flag=True
)
def reset(clean=False):
    """Reset local services (stop and restart)."""
    exit_code = docker_down(clean)
    if exit_code != 0:
        return exit_code
    return docker_up()


@services()
@angreal.command(name="clean", about="stop and remove services including volumes")
def clean():
    """Stop and remove services including volumes."""
    return docker_clean()
