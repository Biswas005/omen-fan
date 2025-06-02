#!/usr/bin/env python3
"""
HP Omen ACPI Interface Tool for Linux
Direct ACPI method calling to control HP Omen hardware
Equivalent to Windows WMI BIOS interface functionality
"""

import os
import sys
import struct
import subprocess
import tempfile
from pathlib import Path
from typing import Optional, List, Dict, Any, Tuple

class OmenACPIInterface:
    def __init__(self):
        # HP ACPI method constants (from reverse engineering)
        self.HP_WMI_COMMAND_GUID = "95764E09-FB56-4E83-B31A-37761F60994A"

        # Command constants from OmenMon BiosData.cs
        self.CMD_DEFAULT = 0x20008      # Standard BIOS command (131080)
        self.CMD_KEYBOARD = 0x20009     # Keyboard backlight command (131081)
        self.CMD_THERMAL = 0x2000A      # Thermal/fan control command (131082)
        self.CMD_GPU_MODE = 0x2000B     # Graphics mode switch command (131083)

        # Authorization signature
        self.SIGN = bytes([0x53, 0x45, 0x43, 0x55])  # "SECU"

        # Fan mode values (corrected from OmenMon BiosData.cs)
        self.FAN_MODES = {
            "default": 0x00,     # Default/Auto thermal profile
            "performance": 0x01, # Performance thermal profile
            "cool": 0x02,        # Cool thermal profile
            "quiet": 0x03,       # Quiet thermal profile
        }

        # ACPI device paths
        self.acpi_devices = []
        self.hp_wmi_path = None

    def check_root(self) -> bool:
        """Check if running as root"""
        return os.geteuid() == 0

    def find_acpi_devices(self) -> List[str]:
        """Find PNP0C14 ACPI devices"""
        devices = []
        acpi_path = "/sys/bus/acpi/devices"

        if not os.path.exists(acpi_path):
            return devices

        try:
            for device_dir in os.listdir(acpi_path):
                device_path = os.path.join(acpi_path, device_dir)
                hid_path = os.path.join(device_path, "hid")

                if os.path.exists(hid_path):
                    try:
                        with open(hid_path, 'r') as f:
                            hid = f.read().strip()
                            if "PNP0C14" in hid:
                                devices.append(device_dir)
                    except (OSError, IOError):
                        pass
        except (OSError, IOError):
            pass

        return devices

    def find_hp_wmi_device(self) -> Optional[str]:
        """Find HP WMI platform device"""
        hp_wmi_paths = [
            "/sys/devices/platform/hp-wmi",
            "/sys/bus/platform/devices/hp-wmi"
        ]

        for path in hp_wmi_paths:
            if os.path.exists(path):
                return path

        return None

    def create_bios_packet(self, cmd: int, data: bytes = b'') -> bytes:
        """Create BIOS data packet with signature and command"""
        # Structure: [4-byte signature][4-byte command][data]
        packet = self.SIGN + struct.pack('<I', cmd) + data

        # Pad to ensure minimum size
        if len(packet) < 128:
            packet += b'\x00' * (128 - len(packet))

        return packet

    def write_acpi_method_call(self, method_name: str, args: bytes) -> Optional[bytes]:
        """Write ACPI method call using acpi_call kernel module"""
        acpi_call_path = "/proc/acpi/call"

        if not os.path.exists(acpi_call_path):
            print("acpi_call kernel module not loaded. Try: sudo modprobe acpi_call")
            return None

        try:
            # Format: method_name arg1 arg2 ...
            # For binary data, we need to format as hex
            hex_args = ' '.join([f'0x{b:02x}' for b in args])
            call_string = f"{method_name} {hex_args}"

            # Write to acpi_call
            with open(acpi_call_path, 'w') as f:
                f.write(call_string)

            # Read result
            with open(acpi_call_path, 'r') as f:
                result = f.read().strip()

            return result.encode() if result else None

        except (OSError, IOError, PermissionError) as e:
            print(f"Error calling ACPI method: {e}")
            return None

    def test_hp_wmi_methods(self) -> Dict[str, Any]:
        """Test various HP WMI method calls"""
        results = {}

        # Common HP ACPI method names (from DSDT analysis)
        hp_methods = [
            "\\_SB.WMID.HWMC",  # Hardware Management Controller
            "\\_SB.WMI1.WQBA",  # WMI Query Block A
            "\\_SB.AMW0.WQBA",  # ASUS WMI (sometimes used by HP)
            "\\_SB.WMID.WMBB",  # WMI Method Block B
        ]

        for method in hp_methods:
            print(f"Testing ACPI method: {method}")

            # Try with default command
            packet = self.create_bios_packet(self.CMD_DEFAULT)
            result = self.write_acpi_method_call(method, packet)

            results[method] = {
                "accessible": result is not None,
                "result": result.decode() if result else None
            }

            if result:
                print(f"  ✓ Method callable, result: {result[:50]}...")
            else:
                print(f"  ✗ Method not accessible or no result")

        return results

    def test_fan_control_acpi(self) -> Dict[str, Any]:
        """Test ACPI-based fan control"""
        results = {}

        # Test different fan modes
        for mode_name, mode_value in self.FAN_MODES.items():
            print(f"Testing fan mode: {mode_name} (value: 0x{mode_value:02X})")

            # Create packet with fan mode data using thermal command
            data = struct.pack('B', mode_value) + b'\x00' * 3  # Pad to 4 bytes
            packet = self.create_bios_packet(self.CMD_THERMAL, data)

            # Try HP-specific method calls
            hp_fan_methods = [
                "\\_SB.WMID.HWMC",
                "\\_TZ.FAN0._FST",  # Fan status
                "\\_TZ.FAN1._FST",  # Fan status
            ]

            mode_results = {}
            for method in hp_fan_methods:
                result = self.write_acpi_method_call(method, packet)
                mode_results[method] = result is not None

            results[mode_name] = mode_results

        return results

    def dump_acpi_tables(self) -> List[str]:
        """Dump ACPI tables to find HP-specific methods"""
        tables = []
        acpi_tables_path = "/sys/firmware/acpi/tables"

        if not os.path.exists(acpi_tables_path):
            return tables

        try:
            for table_file in os.listdir(acpi_tables_path):
                if table_file.startswith(("DSDT", "SSDT")):
                    table_path = os.path.join(acpi_tables_path, table_file)

                    # Try to read and analyze table
                    try:
                        with open(table_path, 'rb') as f:
                            table_data = f.read()

                        # Look for HP/WMI-related strings
                        table_str = str(table_data)
                        if any(term in table_str for term in ['WMID', 'HWMC', 'OMEN', 'HP']):
                            tables.append(table_file)

                    except (OSError, IOError):
                        pass

        except (OSError, IOError):
            pass

        return tables

    def try_direct_wmi_access(self) -> Dict[str, Any]:
        """Try direct WMI device access"""
        results = {}
        wmi_path = "/sys/bus/wmi/devices"

        if not os.path.exists(wmi_path):
            return results

        try:
            for wmi_device in os.listdir(wmi_path):
                device_path = os.path.join(wmi_path, wmi_device)

                # Check if device has HP-related methods
                methods_found = []

                for item in os.listdir(device_path):
                    item_path = os.path.join(device_path, item)

                    # Look for method-like files
                    if os.path.isfile(item_path) and not item.startswith('.'):
                        # Try to read GUID
                        if item == "guid":
                            try:
                                with open(item_path, 'r') as f:
                                    guid = f.read().strip()
                                    results[wmi_device] = {"guid": guid}
                            except (OSError, IOError):
                                pass

                        # Check for writable method files
                        if os.access(item_path, os.W_OK):
                            methods_found.append(item)

                if methods_found:
                    if wmi_device not in results:
                        results[wmi_device] = {}
                    results[wmi_device]["writable_methods"] = methods_found

        except (OSError, IOError):
            pass

        return results

    def test_hwmon_fan_control(self) -> Dict[str, Any]:
        """Test hardware monitoring fan control"""
        results = {}
        hwmon_path = "/sys/class/hwmon"

        if not os.path.exists(hwmon_path):
            return results

        try:
            for hwmon_device in os.listdir(hwmon_path):
                device_path = os.path.join(hwmon_path, hwmon_device)

                # Check device name
                name_path = os.path.join(device_path, "name")
                device_name = "unknown"

                if os.path.exists(name_path):
                    try:
                        with open(name_path, 'r') as f:
                            device_name = f.read().strip()
                    except (OSError, IOError):
                        pass

                # Look for HP-specific devices
                if device_name.lower() in ["hp", "omen"] or "hp" in device_name.lower():
                    device_info = {"name": device_name, "controls": []}

                    # Find fan and PWM controls
                    for control_file in os.listdir(device_path):
                        if control_file.startswith(("fan", "pwm")):
                            control_path = os.path.join(device_path, control_file)

                            if os.path.isfile(control_path):
                                control_info = {
                                    "file": control_file,
                                    "readable": os.access(control_path, os.R_OK),
                                    "writable": os.access(control_path, os.W_OK)
                                }

                                # Try to read current value
                                if control_info["readable"]:
                                    try:
                                        with open(control_path, 'r') as f:
                                            control_info["current_value"] = f.read().strip()
                                    except (OSError, IOError):
                                        pass

                                device_info["controls"].append(control_info)

                    if device_info["controls"]:
                        results[hwmon_device] = device_info

        except (OSError, IOError):
            pass

        return results

    def set_fan_mode(self, mode: str) -> bool:
        """Set fan mode using ACPI method with correct OmenMon values"""
        if mode not in self.FAN_MODES:
            print(f"Invalid fan mode. Available modes: {list(self.FAN_MODES.keys())}")
            return False

        mode_value = self.FAN_MODES[mode]
        print(f"Setting fan mode to: {mode} (value: 0x{mode_value:02X})")

        # Create packet with thermal control command and fan mode data
        # Structure based on OmenMon: [signature][command][fan_mode][padding]
        data = struct.pack('B', mode_value) + b'\x00' * 3  # Pad to 4 bytes
        packet = self.create_bios_packet(self.CMD_THERMAL, data)

        # Try HP-specific method calls in order of preference
        hp_fan_methods = [
            "\\_SB.WMID.HWMC",  # Primary HP WMI method
            "\\_SB.WMI1.WQBA",  # Alternative WMI method
            "\\_SB.WMID.WMBB",  # Backup WMI method
        ]

        for method in hp_fan_methods:
            print(f"Trying method: {method}")
            result = self.write_acpi_method_call(method, packet)
            if result:
                print(f"✓ Fan mode set successfully using {method}")
                return True
            else:
                print(f"✗ Method {method} failed or returned no result")

        # Try fallback with default command
        print("Trying fallback with CMD_DEFAULT...")
        packet_fallback = self.create_bios_packet(self.CMD_DEFAULT, data)
        for method in hp_fan_methods:
            result = self.write_acpi_method_call(method, packet_fallback)
            if result:
                print(f"✓ Fan mode set successfully using {method} (fallback)")
                return True

        print("✗ Failed to set fan mode with all methods")
        return False

    def get_system_info(self) -> Dict[str, Any]:
        """Get system information"""
        info = {}

        # DMI information
        dmi_path = "/sys/class/dmi/id"
        if os.path.exists(dmi_path):
            dmi_files = ["product_name", "sys_vendor", "bios_version", "bios_date"]
            for file in dmi_files:
                file_path = os.path.join(dmi_path, file)
                if os.path.exists(file_path):
                    try:
                        with open(file_path, 'r') as f:
                            info[file] = f.read().strip()
                    except (OSError, IOError):
                        pass

        return info

    def run_comprehensive_test(self) -> None:
        """Run comprehensive HP Omen ACPI interface test"""
        print("HP Omen ACPI Interface Tool")
        print("=" * 40)

        # System info
        print("=== System Information ===")
        sys_info = self.get_system_info()
        for key, value in sys_info.items():
            print(f"{key}: {value}")

        if not self.check_root():
            print("\n⚠️  Warning: Not running as root. Most ACPI operations will fail.")
            print("   Run with sudo for full functionality.\n")

        # 1. Find ACPI devices
        print("=== Finding ACPI Devices ===")
        self.acpi_devices = self.find_acpi_devices()

        if self.acpi_devices:
            print(f"✓ Found {len(self.acpi_devices)} PNP0C14 devices:")
            for device in self.acpi_devices:
                print(f"  {device}")
        else:
            print("✗ No PNP0C14 ACPI devices found")
            return

        # 2. Find HP WMI device
        print(f"\n=== Finding HP WMI Device ===")
        self.hp_wmi_path = self.find_hp_wmi_device()

        if self.hp_wmi_path:
            print(f"✓ HP WMI device found: {self.hp_wmi_path}")
        else:
            print("✗ HP WMI device not found")

        # 3. Check for acpi_call module
        print(f"\n=== Checking ACPI Call Interface ===")
        if os.path.exists("/proc/acpi/call"):
            print("✓ acpi_call module loaded")

            # Test HP WMI methods
            print(f"\n=== Testing HP WMI ACPI Methods ===")
            wmi_results = self.test_hp_wmi_methods()

            accessible_methods = [m for m, r in wmi_results.items() if r["accessible"]]
            if accessible_methods:
                print(f"✓ Found {len(accessible_methods)} accessible ACPI methods")
            else:
                print("✗ No accessible ACPI methods found")

            # Test fan control
            print(f"\n=== Testing ACPI Fan Control ===")
            fan_results = self.test_fan_control_acpi()
            print("Fan mode test results:")
            for mode, methods in fan_results.items():
                working_methods = sum(1 for accessible in methods.values() if accessible)
                print(f"  {mode}: {working_methods}/{len(methods)} methods accessible")

        else:
            print("✗ acpi_call module not loaded")
            print("   Load with: sudo modprobe acpi_call")

        # 4. Direct WMI access test
        print(f"\n=== Testing Direct WMI Access ===")
        wmi_access = self.try_direct_wmi_access()

        if wmi_access:
            print(f"✓ Found {len(wmi_access)} WMI devices with methods")
            for device, info in wmi_access.items():
                if "writable_methods" in info:
                    print(f"  {device}: {len(info['writable_methods'])} writable methods")
        else:
            print("✗ No accessible WMI devices found")

        # 5. Hardware monitoring test
        print(f"\n=== Testing Hardware Monitoring Fan Control ===")
        hwmon_results = self.test_hwmon_fan_control()

        if hwmon_results:
            print(f"✓ Found {len(hwmon_results)} HP hwmon devices")
            for device, info in hwmon_results.items():
                print(f"  {device} ({info['name']}): {len(info['controls'])} controls")
                for control in info['controls']:
                    status = []
                    if control['readable']:
                        status.append("R")
                    if control['writable']:
                        status.append("W")
                    status_str = "/".join(status) if status else "none"
                    print(f"    {control['file']}: {status_str}")
                    if 'current_value' in control:
                        print(f"      Current: {control['current_value']}")
        else:
            print("✗ No HP hardware monitoring devices with fan control found")

        # 6. ACPI table analysis
        print(f"\n=== ACPI Table Analysis ===")
        hp_tables = self.dump_acpi_tables()

        if hp_tables:
            print(f"✓ Found {len(hp_tables)} ACPI tables with HP/WMI references:")
            for table in hp_tables:
                print(f"  {table}")
        else:
            print("✗ No ACPI tables with HP/WMI references found")

        print(f"\n=== Test Complete ===")
        print("Use the following methods to control your HP Omen:")
        print("  • ACPI methods (requires acpi_call module)")
        print("  • WMI device files (if available)")
        print("  • Hardware monitoring controls (hwmon)")


def main():
    """Main function with command line interface"""
    if len(sys.argv) < 2:
        print("HP Omen ACPI Interface Tool")
        print("Usage:")
        print(f"  {sys.argv[0]} test          - Run comprehensive test")
        print(f"  {sys.argv[0]} fan <mode>    - Set fan mode")
        print(f"  {sys.argv[0]} info          - Show system info")
        print()
        print("Fan modes: default, performance, cool, quiet")
        sys.exit(1)

    omen = OmenACPIInterface()
    command = sys.argv[1].lower()

    if command == "test":
        omen.run_comprehensive_test()
    elif command == "fan":
        if len(sys.argv) < 3:
            print("Usage: fan <mode>")
            print(f"Available modes: {list(omen.FAN_MODES.keys())}")
            sys.exit(1)

        if not omen.check_root():
            print("Error: Fan control requires root privileges")
            print("Run with: sudo python3 hp_omen_acpi.py fan <mode>")
            sys.exit(1)

        mode = sys.argv[2].lower()
        success = omen.set_fan_mode(mode)
        sys.exit(0 if success else 1)
    elif command == "info":
        sys_info = omen.get_system_info()
        print("System Information:")
        for key, value in sys_info.items():
            print(f"  {key}: {value}")
    else:
        print(f"Unknown command: {command}")
        sys.exit(1)


if __name__ == "__main__":
    main()
