def cmd_config(args):
    """Show current configuration."""
    config_path = get_config_path()

    print(f"\nConfig file: {config_path}")
    print("-" * 60)

    try:
        print(config_path.read_text())
    except Exception as e:
        print(f"Error reading config: {e}", file=sys.stderr)
        return 1

    return 0

