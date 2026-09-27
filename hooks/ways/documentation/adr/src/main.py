# ============================================================================
# Main
# ============================================================================

def main():
    parser = argparse.ArgumentParser(
        description='ADR - Architecture Decision Record CLI Tool',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__
    )
    parser.add_argument('--version', action='version',
                        version=f'adr-tool {TOOL_VERSION}')
    subparsers = parser.add_subparsers(dest='command', help='Command')

    # list
    p_list = subparsers.add_parser('list', aliases=['ls'], help='List ADRs')
    p_list.add_argument('--domain', '-d', help='Filter by domain')
    p_list.add_argument('--status', '-s', help='Filter by status')
    p_list.add_argument('--group', '-g', action='store_true',
                        help='Group by domain')
    list_scope = p_list.add_mutually_exclusive_group()
    list_scope.add_argument('--archived', action='store_true',
                            help='List archived ADRs only')
    list_scope.add_argument('--all', action='store_true',
                            help='List active and archived ADRs')

    # view
    p_view = subparsers.add_parser('view', aliases=['v', 'show'], help='View an ADR')
    p_view.add_argument('adr', help='ADR number (e.g., 38, 038, ADR-038)')

    # new
    p_new = subparsers.add_parser('new', help='Create new ADR')
    p_new.add_argument('domain', help='Domain (see `adr domains` for list)')
    p_new.add_argument('title', help='ADR title')
    p_new.add_argument('--kind', help='adr/v1: record kind (default: decision)')
    p_new.add_argument('--verb', help='adr/v1: decision verb (add, cut, change, retire, constrain)')
    p_new.add_argument('--capability', help='adr/v1: capability from the adr.yaml vocabulary')
    p_new.add_argument('--agent', help='adr/v1: the agent writing the record (e.g. Claude)')
    p_new.add_argument('--model', help='adr/v1: the model the agent runs on')

    # rename
    p_rename = subparsers.add_parser('rename', help='Rename an ADR title and/or file slug')
    p_rename.add_argument('adr', help='ADR number (e.g., 302, ADR-302)')
    p_rename.add_argument('title', nargs='?', help='New title (updates the # ADR-NNN: heading)')
    p_rename.add_argument('--slug', help='Override the filename slug (default: derived from title)')

    # lint
    p_lint = subparsers.add_parser('lint', help='Lint ADR files')
    p_lint.add_argument('paths', nargs='*', help='Specific files to lint')
    p_lint.add_argument('--check', action='store_true', help='Exit 1 if errors (CI mode)')

    # index
    index_parser = subparsers.add_parser('index', help='Generate ADR index')
    index_parser.add_argument('-y', '--yes', action='store_true',
                              help='Update without prompting')

    # domains
    subparsers.add_parser('domains', help='List domain number series')

    # archive
    p_archive = subparsers.add_parser(
        'archive', help='Archive an ADR out of the active set')
    p_archive.add_argument('adr', help='ADR number (e.g., 38, 038, ADR-038)')
    p_archive.add_argument('--reason', required=True,
                           help='Why this ADR leaves the active set (recorded in the banner)')
    p_archive.add_argument('--superseded-by', dest='superseded_by',
                           help='Superseding ADR(s), comma-separated (e.g., ADR-51 or 51,52#4)')
    p_archive.add_argument('--status',
                           help='Archive status (default: Superseded; validated against adr.yaml)')
    p_archive.add_argument('--dry-run', action='store_true',
                           help='Report what would change without changing it')

    # config
    subparsers.add_parser('config', help='Show configuration')

    # lifecycle (adr/v1, ADR-304 §2)
    p_accept = subparsers.add_parser('accept', help='Accept a proposed adr/v1 record')
    p_accept.add_argument('adr', help='ADR number (e.g., 101, ADR-101)')
    p_accept.add_argument('--dry-run', action='store_true', help='Check and report without writing')
    for verb, help_text in (('reject', 'Reject a proposed adr/v1 record: considered and declined'),
                            ('abandon', 'Abandon a proposed adr/v1 record: dropped before a decision')):
        p_close = subparsers.add_parser(verb, help=help_text)
        p_close.add_argument('adr', help='ADR number (e.g., 101, ADR-101)')
        p_close.add_argument('--reason', help='Why (required; appended as a Closure section)')
        p_close.add_argument('--dry-run', action='store_true', help='Report without writing')

    # cite
    p_cite = subparsers.add_parser('cite', help='Check ADR citations in code against the records')
    p_cite.add_argument('paths', nargs='*', help='Limit the scan to these files or directories')
    p_cite.add_argument('--check', action='store_true', help='Exit 1 if errors (CI mode)')
    p_cite.add_argument('--no-inventory', action='store_true',
                        help="Skip surface inventories (they run shell commands from adr.yaml)")

    args = parser.parse_args()

    if not args.command:
        parser.print_help()
        return 0

    commands = {
        'list': cmd_list,
        'ls': cmd_list,
        'view': cmd_view,
        'v': cmd_view,
        'show': cmd_view,
        'new': cmd_new,
        'rename': cmd_rename,
        'lint': cmd_lint,
        'index': cmd_index,
        'archive': cmd_archive,
        'domains': cmd_domains,
        'config': cmd_config,
        'cite': cmd_cite,
        'accept': cmd_accept,
        'reject': cmd_reject,
        'abandon': cmd_abandon,
    }

    return commands[args.command](args)

if __name__ == '__main__':
    sys.exit(main())
