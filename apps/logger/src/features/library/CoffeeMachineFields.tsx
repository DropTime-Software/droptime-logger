/**
 * features/library — coffee/machine autocomplete fields for SetupScreen
 * (CONTRACTS §8, owner: librarian).
 *
 * Mount point (pre-placed by integration): rendered inside SetupScreen's field
 * grid. Upgrades the plain free-text coffee/machine inputs into type-ahead
 * pickers over the local library (list_coffees / list_machines) with inline
 * "create" — resolving coffeeLocalId / machineLocalId alongside the display
 * name. Name/id round-trip through SetupScreen's local state into
 * StartConfig → start_session meta WITHOUT touching SetupScreen.
 *
 * Picking a saved machine resolves its `machineLocalId` (a library link only).
 * The engine does NOT resolve source_pin from machineLocalId — a device session
 * needs the SourcePin passed explicitly on StartConfig.sourcePin. SetupScreen
 * surfaces saved TC4 roasters as device sources and threads that pin through;
 * this field only round-trips the display name + library id.
 *
 * In browser demo mode the library backend is absent, so both fields render as
 * plain free-text inputs (empty suggestion lists, no create) — same wiring.
 */
import { Field } from '../../components/ui';
import { Combobox } from './Combobox';
import { useLibrary } from './useLibrary';

export interface CoffeeMachineFieldsProps {
  coffeeName: string;
  onCoffeeNameChange: (name: string) => void;
  machineName: string;
  onMachineNameChange: (name: string) => void;
  /** resolved coffees_local id, undefined when free-text/unsaved */
  coffeeLocalId?: number;
  onCoffeeLocalIdChange: (id: number | undefined) => void;
  /** resolved machines_local id, undefined when free-text/unsaved */
  machineLocalId?: number;
  onMachineLocalIdChange: (id: number | undefined) => void;
}

export function CoffeeMachineFields(props: CoffeeMachineFieldsProps) {
  const { coffees, machines, loading, tauri, createCoffee, createMachine } = useLibrary();

  return (
    <>
      <Field
        label="Coffee"
        htmlFor="coffee-lib"
        hint={tauri ? 'Type to search your library or add a new coffee.' : undefined}
      >
        <Combobox
          id="coffee-lib"
          value={props.coffeeName}
          selectedId={props.coffeeLocalId}
          items={coffees}
          loading={loading}
          placeholder="e.g. Ethiopia Guji"
          onCreate={tauri ? createCoffee : undefined}
          createLabel={(name) => `Add “${name}” to library`}
          onChange={(value, id) => {
            props.onCoffeeNameChange(value);
            props.onCoffeeLocalIdChange(id);
          }}
        />
      </Field>

      <Field
        label="Machine"
        htmlFor="machine-lib"
        hint={tauri ? 'Type to search your library or add a new machine.' : 'Optional.'}
      >
        <Combobox
          id="machine-lib"
          value={props.machineName}
          selectedId={props.machineLocalId}
          items={machines}
          loading={loading}
          placeholder="e.g. Loring S15"
          onCreate={tauri ? (name) => createMachine(name) : undefined}
          createLabel={(name) => `Add “${name}” to library`}
          onChange={(value, id) => {
            props.onMachineNameChange(value);
            props.onMachineLocalIdChange(id);
          }}
        />
      </Field>
    </>
  );
}
