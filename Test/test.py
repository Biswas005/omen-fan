#!/usr/bin/env python3
"""
HP Omen Linux ACPI/WMI Interface Tester
Tests for HP Omen BIOS functionality on Linux systems
Checks ACPI interfaces, kernel modules, and sysfs entries
"""

import os
import sys
import glob
import struct
import subprocess
from pathlib import Path
from typing import Optional, List, Dict, Any, Tuple
import re

class OmenLinuxTester:
    def __init__(self):
        # HP-specific identifiers
        self.HP_WMI_GUID = "95764E09-FB56-4E83-B31A-37761F60994A"  # Common HP WMI GUID
        self.OMEN_WMI_GUID = "ABBC0F6B-8EA1-11D1-00A0-C90629100000"  # Alternative GUID
        
        # ACPI device identifiers
        self.ACPI_DEVICES = [
            "PNP0C14",  # Windows Management Instrumentation for ACPI
            "HPQ6001",  # HP Hotkey
            "HPQ6007",  # HP WMI
            "ACPI0012", # NVDIMM Root Device
        ]
        
        # HP-specific kernel modules
        self.HP_MODULES = [
            "hp-wmi",
            "hp_accel", 
            "hp-wireless",
            "omen-wmi",
            "platform_profile"
        ]
        
        # Command constants (from original)
        self.CMD_DEFAULT = 0x20008
        self.CMD_KEYBOARD = 0x20009  
        self.CMD_LEGACY = 0x00001
        self.CMD_GPU_MODE = 0x00002
        
        # Authorization signature
        self.SIGN = bytes([0x53, 0x45, 0x43, 0x55])  # "SECU"
        
    def check_root_privileges(self) -> bool:
        """Check if running with root privileges"""
        return os.geteuid() == 0
    
    def run_command(self, cmd: List[str]) -> Tuple[int, str, str]:
        """Run shell command and return exit code, stdout, stderr"""
        try:
            result = subprocess.run(
                cmd, 
                capture_output=True, 
                text=True, 
                timeout=10
            )
            return result.returncode, result.stdout, result.stderr
        except subprocess.TimeoutExpired:
            return -1, "", "Command timeout"
        except Exception as e:
            return -1, "", str(e)
    
    def scan_dmi_info(self) -> Dict[str, str]:
        """Scan DMI/SMBIOS information"""
        dmi_info = {}
        dmi_fields = [
            "sys_vendor", "product_name", "product_version", 
            "board_vendor", "board_name", "bios_vendor", "bios_version"
        ]
        
        for field in dmi_fields:
            dmi_path = f"/sys/class/dmi/id/{field}"
            try:
                if os.path.exists(dmi_path):
                    with open(dmi_path, 'r') as f:
                        dmi_info[field] = f.read().strip()
            except (OSError, IOError):
                dmi_info[field] = "N/A"
        
        return dmi_info
    
    def scan_acpi_devices(self) -> List[Dict[str, Any]]:
        """Scan for ACPI devices"""
        devices = []
        acpi_path = "/sys/bus/acpi/devices"
        
        if not os.path.exists(acpi_path):
            return devices
        
        try:
            for device_dir in os.listdir(acpi_path):
                device_path = os.path.join(acpi_path, device_dir)
                device_info = {"device_id": device_dir}
                
                # Read HID (Hardware ID)
                hid_path = os.path.join(device_path, "hid")
                if os.path.exists(hid_path):
                    try:
                        with open(hid_path, 'r') as f:
                            device_info["hid"] = f.read().strip()
                    except (OSError, IOError):
                        device_info["hid"] = "N/A"
                
                # Read status
                status_path = os.path.join(device_path, "status")
                if os.path.exists(status_path):
                    try:
                        with open(status_path, 'r') as f:
                            status = int(f.read().strip())
                            device_info["status"] = f"0x{status:X}"
                            device_info["enabled"] = bool(status & 0x01)
                    except (OSError, IOError, ValueError):
                        device_info["status"] = "N/A"
                        device_info["enabled"] = False
                
                # Check if it's an HP/Omen relevant device
                hid = device_info.get("hid", "")
                if any(acpi_id in hid for acpi_id in self.ACPI_DEVICES):
                    device_info["relevant"] = True
                    devices.append(device_info)
                    
        except (OSError, IOError) as e:
            print(f"Error scanning ACPI devices: {e}")
        
        return devices
    
    def scan_wmi_devices(self) -> List[Dict[str, Any]]:
        """Scan for WMI devices and GUIDs"""
        wmi_devices = []
        wmi_path = "/sys/bus/wmi/devices"
        
        if not os.path.exists(wmi_path):
            return wmi_devices
        
        try:
            for device_dir in os.listdir(wmi_path):
                device_path = os.path.join(wmi_path, device_dir)
                device_info = {"device": device_dir}
                
                # Read GUID
                guid_path = os.path.join(device_path, "guid")
                if os.path.exists(guid_path):
                    try:
                        with open(guid_path, 'r') as f:
                            guid = f.read().strip()
                            device_info["guid"] = guid
                            
                            # Check if it's HP-specific
                            if guid in [self.HP_WMI_GUID, self.OMEN_WMI_GUID]:
                                device_info["hp_specific"] = True
                    except (OSError, IOError):
                        pass
                
                # Read instance count
                instance_count_path = os.path.join(device_path, "instance_count")
                if os.path.exists(instance_count_path):
                    try:
                        with open(instance_count_path, 'r') as f:
                            device_info["instance_count"] = int(f.read().strip())
                    except (OSError, IOError, ValueError):
                        pass
                
                wmi_devices.append(device_info)
                
        except (OSError, IOError) as e:
            print(f"Error scanning WMI devices: {e}")
        
        return wmi_devices
    
    def check_kernel_modules(self) -> Dict[str, bool]:
        """Check if HP-related kernel modules are loaded"""
        module_status = {}
        
        try:
            with open("/proc/modules", 'r') as f:
                loaded_modules = f.read()
            
            for module in self.HP_MODULES:
                module_status[module] = module in loaded_modules
                
        except (OSError, IOError):
            for module in self.HP_MODULES:
                module_status[module] = False
        
        return module_status
    
    def scan_platform_devices(self) -> List[Dict[str, Any]]:
        """Scan for platform devices related to HP"""
        devices = []
        platform_path = "/sys/bus/platform/devices"
        
        if not os.path.exists(platform_path):
            return devices
        
        try:
            for device_dir in os.listdir(platform_path):
                if any(hp_term in device_dir.lower() for hp_term in ["hp", "omen", "pavilion"]):
                    device_path = os.path.join(platform_path, device_dir)
                    device_info = {"device": device_dir, "path": device_path}
                    
                    # Check for driver
                    driver_link = os.path.join(device_path, "driver")
                    if os.path.islink(driver_link):
                        try:
                            driver_path = os.readlink(driver_link)
                            device_info["driver"] = os.path.basename(driver_path)
                        except OSError:
                            pass
                    
                    devices.append(device_info)
                    
        except (OSError, IOError) as e:
            print(f"Error scanning platform devices: {e}")
        
        return devices
    
    def check_hwmon_sensors(self) -> List[Dict[str, Any]]:
        """Check for hardware monitoring sensors"""
        sensors = []
        hwmon_path = "/sys/class/hwmon"
        
        if not os.path.exists(hwmon_path):
            return sensors
        
        try:
            for hwmon_dir in os.listdir(hwmon_path):
                hwmon_device_path = os.path.join(hwmon_path, hwmon_dir)
                sensor_info = {"device": hwmon_dir}
                
                # Read name
                name_path = os.path.join(hwmon_device_path, "name")
                if os.path.exists(name_path):
                    try:
                        with open(name_path, 'r') as f:
                            name = f.read().strip()
                            sensor_info["name"] = name
                            
                            # Check if it's relevant to HP/thermal management
                            if any(term in name.lower() for term in ["hp", "omen", "thermal", "fan", "temp"]):
                                sensor_info["relevant"] = True
                    except (OSError, IOError):
                        pass
                
                # Scan for temperature and fan inputs
                temp_files = glob.glob(os.path.join(hwmon_device_path, "temp*_input"))
                fan_files = glob.glob(os.path.join(hwmon_device_path, "fan*_input"))
                
                sensor_info["temp_sensors"] = len(temp_files)
                sensor_info["fan_sensors"] = len(fan_files)
                
                if sensor_info.get("relevant") or temp_files or fan_files:
                    sensors.append(sensor_info)
                    
        except (OSError, IOError) as e:
            print(f"Error checking hwmon sensors: {e}")
        
        return sensors
    
    def test_thermal_zones(self) -> List[Dict[str, Any]]:
        """Test thermal zones"""
        thermal_zones = []
        thermal_path = "/sys/class/thermal"
        
        if not os.path.exists(thermal_path):
            return thermal_zones
        
        try:
            for zone_dir in sorted(os.listdir(thermal_path)):
                if zone_dir.startswith("thermal_zone"):
                    zone_path = os.path.join(thermal_path, zone_dir)
                    zone_info = {"zone": zone_dir}
                    
                    # Read type
                    type_path = os.path.join(zone_path, "type")
                    if os.path.exists(type_path):
                        try:
                            with open(type_path, 'r') as f:
                                zone_info["type"] = f.read().strip()
                        except (OSError, IOError):
                            pass
                    
                    # Read temperature
                    temp_path = os.path.join(zone_path, "temp")
                    if os.path.exists(temp_path):
                        try:
                            with open(temp_path, 'r') as f:
                                temp_millicelsius = int(f.read().strip())
                                zone_info["temp_celsius"] = temp_millicelsius / 1000.0
                        except (OSError, IOError, ValueError):
                            pass
                    
                    thermal_zones.append(zone_info)
                    
        except (OSError, IOError) as e:
            print(f"Error checking thermal zones: {e}")
        
        return thermal_zones
    
    def check_platform_profiles(self) -> Optional[Dict[str, Any]]:
        """Check platform profile support (performance modes)"""
        profile_path = "/sys/firmware/acpi/platform_profile"
        
        if not os.path.exists(profile_path):
            return None
        
        profile_info = {}
        
        try:
            # Read current profile
            with open(profile_path, 'r') as f:
                profile_info["current"] = f.read().strip()
                
            # Read available profiles
            choices_path = "/sys/firmware/acpi/platform_profile_choices"
            if os.path.exists(choices_path):
                with open(choices_path, 'r') as f:
                    profile_info["available"] = f.read().strip().split()
                    
        except (OSError, IOError) as e:
            print(f"Error reading platform profiles: {e}")
            return None
        
        return profile_info
    
    def test_wmi_method_call(self, wmi_device: str) -> bool:
        """Test if we can interact with WMI device"""
        # This is a basic test - actual WMI method calls in Linux
        # would require specific kernel module support or direct ACPI calls
        wmi_path = f"/sys/bus/wmi/devices/{wmi_device}"
        
        if not os.path.exists(wmi_path):
            return False
        
        # Check if device has any writable attributes
        try:
            for item in os.listdir(wmi_path):
                item_path = os.path.join(wmi_path, item)
                if os.path.isfile(item_path):
                    try:
                        # Test read access
                        with open(item_path, 'r') as f:
                            f.read(1)
                        return True
                    except (OSError, IOError):
                        continue
        except (OSError, IOError):
            pass
        
        return False
    
    def test_acpi_wmi_methods(self) -> List[Dict[str, Any]]:
        """Test ACPI WMI method calls using direct ACPI interface"""
        results = []
        
        # Try to access ACPI methods through debugfs (requires root)
        acpi_debug_path = "/sys/kernel/debug/acpi"
        if not os.path.exists(acpi_debug_path):
            return results
        
        # Look for WMI-related ACPI methods
        try:
            # Try to find ACPI method files
            for root, dirs, files in os.walk(acpi_debug_path):
                for file in files:
                    if any(wmi_term in file.lower() for wmi_term in ["wmi", "bios", "omen"]):
                        file_path = os.path.join(root, file)
                        results.append({
                            "path": file_path,
                            "name": file,
                            "accessible": os.access(file_path, os.R_OK)
                        })
        except (OSError, IOError, PermissionError):
            pass
        
        return results
    
    def test_hp_wmi_interface(self) -> Dict[str, Any]:
        """Test HP WMI interface through sysfs"""
        hp_wmi_info = {}
        
        # Check HP WMI platform device
        hp_wmi_paths = [
            "/sys/devices/platform/hp-wmi",
            "/sys/bus/platform/devices/hp-wmi"
        ]
        
        for hp_wmi_path in hp_wmi_paths:
            if os.path.exists(hp_wmi_path):
                hp_wmi_info["platform_path"] = hp_wmi_path
                
                # Check available attributes
                try:
                    attributes = []
                    for item in os.listdir(hp_wmi_path):
                        item_path = os.path.join(hp_wmi_path, item)
                        if os.path.isfile(item_path) and not item.startswith('.'):
                            try:
                                # Test if readable
                                with open(item_path, 'r') as f:
                                    content = f.read().strip()
                                    attributes.append({
                                        "name": item,
                                        "readable": True,
                                        "content_preview": content[:50] if content else "empty"
                                    })
                            except (OSError, IOError, UnicodeDecodeError):
                                attributes.append({
                                    "name": item,
                                    "readable": False
                                })
                    
                    hp_wmi_info["attributes"] = attributes
                    
                except (OSError, IOError):
                    pass
                
                break
        
        return hp_wmi_info
    
    def test_fan_control_methods(self) -> Dict[str, Any]:
        """Test various fan control methods available on the system"""
        fan_control = {}
        
        # 1. Check hwmon fan controls
        hwmon_fans = []
        hwmon_path = "/sys/class/hwmon"
        
        if os.path.exists(hwmon_path):
            for hwmon_dir in os.listdir(hwmon_path):
                hwmon_device_path = os.path.join(hwmon_path, hwmon_dir)
                
                # Look for fan controls
                fan_files = glob.glob(os.path.join(hwmon_device_path, "fan*"))
                pwm_files = glob.glob(os.path.join(hwmon_device_path, "pwm*"))
                
                if fan_files or pwm_files:
                    fan_info = {
                        "device": hwmon_dir,
                        "fan_files": [os.path.basename(f) for f in fan_files],
                        "pwm_files": [os.path.basename(f) for f in pwm_files]
                    }
                    
                    # Read name if available
                    name_path = os.path.join(hwmon_device_path, "name")
                    if os.path.exists(name_path):
                        try:
                            with open(name_path, 'r') as f:
                                fan_info["name"] = f.read().strip()
                        except (OSError, IOError):
                            pass
                    
                    # Test reading fan speeds
                    for fan_file in fan_files:
                        if fan_file.endswith("_input"):
                            try:
                                fan_path = os.path.join(hwmon_device_path, fan_file)
                                with open(fan_path, 'r') as f:
                                    speed = int(f.read().strip())
                                    fan_info[f"{fan_file}_rpm"] = speed
                            except (OSError, IOError, ValueError):
                                pass
                    
                    hwmon_fans.append(fan_info)
        
        fan_control["hwmon_devices"] = hwmon_fans
        
        # 2. Check for ACPI fan methods
        acpi_thermal_path = "/proc/acpi"
        if os.path.exists(acpi_thermal_path):
            # This is legacy, but might exist on some systems
            fan_control["acpi_proc_available"] = True
        
        return fan_control
    
    def attempt_acpi_method_call(self) -> Dict[str, Any]:
        """Attempt to call ACPI methods similar to Windows WMI calls"""
        results = {}
        
        # This would require root and specific ACPI method knowledge
        # For now, document what we'd need to do
        results["method"] = "Direct ACPI method calls require:"
        results["requirements"] = [
            "Root privileges",
            "ACPI method names (e.g., \\_SB.WMID.HWMC)",
            "Proper input formatting",
            "Knowledge of device-specific protocols"
        ]
        
        # Check if we can access ACPI methods
        acpi_methods = []
        
        # Try to read DSDT/SSDT for method discovery
        acpi_tables_path = "/sys/firmware/acpi/tables"
        if os.path.exists(acpi_tables_path):
            try:
                tables = os.listdir(acpi_tables_path)
                for table in tables:
                    if table.startswith("DSDT") or table.startswith("SSDT"):
                        table_path = os.path.join(acpi_tables_path, table)
                        if os.path.exists(table_path):
                            acpi_methods.append(table)
            except (OSError, IOError):
                pass
        
        results["acpi_tables_found"] = acpi_methods
        
        return results
    
    def run_full_scan(self) -> None:
        """Run comprehensive scan of HP Omen Linux interface"""
        print("HP Omen Linux ACPI/WMI Interface Tester")
        print("=" * 50)
        
        # Check privileges
        if not self.check_root_privileges():
            print("⚠️  Warning: Not running as root. Some tests may fail.")
            print("   Consider running with sudo for complete testing.\n")
        
        # 1. System Information
        print("=== System Information ===")
        dmi_info = self.scan_dmi_info()
        print(f"Vendor: {dmi_info.get('sys_vendor', 'N/A')}")
        print(f"Product: {dmi_info.get('product_name', 'N/A')}")
        print(f"Version: {dmi_info.get('product_version', 'N/A')}")
        print(f"BIOS: {dmi_info.get('bios_vendor', 'N/A')} {dmi_info.get('bios_version', 'N/A')}")
        
        # Check if this is an HP system
        is_hp_system = any("hp" in str(value).lower() for value in dmi_info.values())
        if is_hp_system:
            print("✓ HP system detected")
        else:
            print("⚠️  Non-HP system - some tests may not be relevant")
        
        # 2. Kernel Modules
        print(f"\n=== HP-Related Kernel Modules ===")
        module_status = self.check_kernel_modules()
        loaded_modules = [mod for mod, loaded in module_status.items() if loaded]
        
        if loaded_modules:
            for module in loaded_modules:
                print(f"✓ {module} - loaded")
        else:
            print("✗ No HP-specific modules loaded")
        
        for module, loaded in module_status.items():
            if not loaded:
                print(f"  {module} - not loaded")
        
        # 3. ACPI Devices
        print(f"\n=== ACPI Devices ===")
        acpi_devices = self.scan_acpi_devices()
        
        if acpi_devices:
            for device in acpi_devices:
                print(f"✓ {device['device_id']}")
                print(f"  HID: {device.get('hid', 'N/A')}")
                print(f"  Status: {device.get('status', 'N/A')}")
                print(f"  Enabled: {device.get('enabled', False)}")
                print()
        else:
            print("✗ No relevant ACPI devices found")
        
        # 4. WMI Devices
        print(f"\n=== WMI Devices ===")
        wmi_devices = self.scan_wmi_devices()
        
        if wmi_devices:
            hp_wmi_found = False
            for device in wmi_devices:
                if device.get("hp_specific"):
                    print(f"✓ HP-specific WMI device: {device['device']}")
                    print(f"  GUID: {device.get('guid', 'N/A')}")
                    print(f"  Instances: {device.get('instance_count', 'N/A')}")
                    hp_wmi_found = True
                    
                    # Test WMI method call
                    if self.test_wmi_method_call(device['device']):
                        print(f"  ✓ Device accessible")
                    else:
                        print(f"  ✗ Device not accessible")
                    print()
            
            if not hp_wmi_found:
                print(f"Found {len(wmi_devices)} WMI devices, but none HP-specific")
                # Show first few for reference
                for device in wmi_devices[:3]:
                    print(f"  {device['device']} - GUID: {device.get('guid', 'N/A')}")
        else:
            print("✗ No WMI devices found")
        
        # 5. Platform Devices
        print(f"\n=== Platform Devices ===")
        platform_devices = self.scan_platform_devices()
        
        if platform_devices:
            for device in platform_devices:
                print(f"✓ {device['device']}")
                if 'driver' in device:
                    print(f"  Driver: {device['driver']}")
        else:
            print("✗ No HP-related platform devices found")
        
        # 6. Hardware Monitoring
        print(f"\n=== Hardware Monitoring Sensors ===")
        sensors = self.check_hwmon_sensors()
        
        if sensors:
            for sensor in sensors:
                print(f"✓ {sensor['device']}")
                if 'name' in sensor:
                    print(f"  Name: {sensor['name']}")
                print(f"  Temperature sensors: {sensor.get('temp_sensors', 0)}")
                print(f"  Fan sensors: {sensor.get('fan_sensors', 0)}")
                if sensor.get('relevant'):
                    print(f"  🔥 Potentially relevant for HP thermal management")
                print()
        else:
            print("✗ No relevant hardware monitoring sensors found")
        
        # 7. Thermal Zones
        print(f"\n=== Thermal Zones ===")
        thermal_zones = self.test_thermal_zones()
        
        if thermal_zones:
            for zone in thermal_zones:
                print(f"✓ {zone['zone']}")
                if 'type' in zone:
                    print(f"  Type: {zone['type']}")
                if 'temp_celsius' in zone:
                    print(f"  Temperature: {zone['temp_celsius']:.1f}°C")
                print()
        else:
            print("✗ No thermal zones found")
        
        # 8. Platform Profiles (Performance Modes)
        print(f"\n=== Platform Profiles (Performance Modes) ===")
        profile_info = self.check_platform_profiles()
        
        if profile_info:
            print(f"✓ Platform profile support available")
            print(f"  Current: {profile_info.get('current', 'N/A')}")
            print(f"  Available: {', '.join(profile_info.get('available', []))}")
        else:
            print("✗ No platform profile support found")
        
        # 9. HP WMI Interface Testing
        print(f"\n=== HP WMI Interface Testing ===")
        hp_wmi_info = self.test_hp_wmi_interface()
        
        if hp_wmi_info:
            print(f"✓ HP WMI platform device found")
            if 'platform_path' in hp_wmi_info:
                print(f"  Path: {hp_wmi_info['platform_path']}")
            
            if 'attributes' in hp_wmi_info:
                print(f"  Available attributes:")
                for attr in hp_wmi_info['attributes']:
                    status = "readable" if attr['readable'] else "not readable"
                    print(f"    {attr['name']} - {status}")
                    if attr['readable'] and 'content_preview' in attr:
                        print(f"      Preview: {attr['content_preview']}")
        else:
            print("✗ HP WMI interface not accessible")
        
        # 10. Fan Control Testing
        print(f"\n=== Fan Control Testing ===")
        fan_control = self.test_fan_control_methods()
        
        if fan_control.get('hwmon_devices'):
            print(f"✓ Found {len(fan_control['hwmon_devices'])} hwmon devices with fan control")
            for fan_dev in fan_control['hwmon_devices']:
                print(f"  {fan_dev['device']}")
                if 'name' in fan_dev:
                    print(f"    Name: {fan_dev['name']}")
                if fan_dev['fan_files']:
                    print(f"    Fan sensors: {', '.join(fan_dev['fan_files'])}")
                if fan_dev['pwm_files']:
                    print(f"    PWM controls: {', '.join(fan_dev['pwm_files'])}")
                
                # Show actual fan speeds
                for key, value in fan_dev.items():
                    if key.endswith('_rpm'):
                        print(f"    {key}: {value} RPM")
                print()
        else:
            print("✗ No fan control interfaces found")
        
        # 11. ACPI Method Testing
        print(f"\n=== ACPI Method Access ===")
        acpi_info = self.attempt_acpi_method_call()
        
        if acpi_info.get('acpi_tables_found'):
            print(f"✓ Found ACPI tables: {', '.join(acpi_info['acpi_tables_found'])}")
            print("  These contain the ACPI methods we need to access")
        else:
            print("✗ Cannot access ACPI tables")
        
        print(f"\nFor direct ACPI method calls (equivalent to Windows WMI):")
        print(f"  - Root privileges required")
        print(f"  - Need to identify specific ACPI method names")
        print(f"  - May require custom kernel module or ACPI call interface")
        
        print(f"\n=== Scan Complete ===")
        
        # Summary
        print(f"\n=== Summary ===")
        if is_hp_system:
            print("✓ HP system detected")
        if loaded_modules:
            print(f"✓ {len(loaded_modules)} HP kernel modules loaded")
        if acpi_devices:
            print(f"✓ {len(acpi_devices)} relevant ACPI devices found")
        if any(d.get('hp_specific') for d in wmi_devices):
            print("✓ HP-specific WMI devices found")
        if profile_info:
            print("✓ Platform profile support available")
        
        if not any([is_hp_system, loaded_modules, acpi_devices]):
            print("⚠️  Limited HP Omen support detected on this system")
            print("   This may not be an HP Omen device, or drivers may not be loaded")

def main():
    """Main entry point"""
    if len(sys.argv) > 1 and sys.argv[1] in ["--help", "-h"]:
        print("HP Omen Linux ACPI/WMI Interface Tester")
        print("Usage: python3 omen_linux_tester.py")
        print("\nThis script tests Linux interfaces for HP Omen systems")
        print("including ACPI devices, WMI interfaces, kernel modules,")
        print("and hardware monitoring capabilities.")
        print("\nFor complete testing, run with sudo privileges.")
        return
    
    try:
        tester = OmenLinuxTester()
        tester.run_full_scan()
    except KeyboardInterrupt:
        print("\nScan interrupted by user")
    except Exception as e:
        print(f"Unexpected error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)

if __name__ == "__main__":
    main()
